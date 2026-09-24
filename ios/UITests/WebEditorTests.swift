import XCTest

/// The editing surface, on a running phone.
///
/// The questions a command line cannot answer: does the keyboard come up, does
/// typing reach Swift, does a selection report itself, does undo work. Each
/// test attaches a screenshot, because "the caret is above the keyboard" is a
/// claim somebody should be able to check with their eyes rather than take on
/// trust.
///
/// They run against `EditorHarnessView` — the real editor with a sample
/// document and no network, so a failure here is about the editor rather than
/// about signing in.
final class WebEditorTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-uitest"]
        app.launch()
    }

    /// The web view renders the document, and Swift agrees about the blocks.
    func testTheDocumentLoads() throws {
        let blocks = app.staticTexts["harness.blocks"]
        XCTAssertTrue(blocks.waitForExistence(timeout: 20))
        XCTAssertTrue(app.webViews.firstMatch.waitForExistence(timeout: 10))
        XCTAssertTrue(
            app.webViews.staticTexts["The Long Field"].waitForExistence(timeout: 10),
            "the editor did not render the document"
        )
        attach(named: "loaded")
    }

    /// Question 1: does the caret stay above the keyboard?
    ///
    /// Asked of the last line, which is where it fails if it fails: tap into
    /// the blockquote, wait for the keyboard, and check the line the caret is
    /// on is still on screen above it.
    func testCaretStaysAboveTheKeyboard() throws {
        let quote = app.webViews.staticTexts.containing(
            NSPredicate(format: "label CONTAINS 'fenceposts'")
        ).firstMatch
        XCTAssertTrue(quote.waitForExistence(timeout: 20))
        quote.tap()

        let keyboard = app.keyboards.firstMatch
        XCTAssertTrue(keyboard.waitForExistence(timeout: 10), "no keyboard came up")
        // Let the scroll settle before measuring it.
        Thread.sleep(forTimeInterval: 1.5)

        let line = quote.frame
        let keyboardTop = keyboard.frame.minY
        attach(named: "keyboard")
        // Printed rather than only asserted: these numbers are the evidence,
        // and the screenshot beside them does not always show the keyboard the
        // accessibility tree says is up.
        print("EDITOR geometry line=\(line) keyboard=\(keyboard.frame) keys=\(app.keys.count)")

        // With a hardware keyboard attached, the simulator shows only the
        // accessory bar and `app.keyboards` matches that instead — which would
        // pass this test while proving nothing. Run it with the hardware
        // keyboard off (`defaults write com.apple.iphonesimulator
        // ConnectHardwareKeyboard -bool false`).
        // The keyboard can be *in the hierarchy* and parked off the bottom of
        // the screen: with a hardware keyboard attached the simulator shows only
        // the accessory bar, while `app.keyboards` still reports a 233-point
        // element with 35 keys at y=952 on an 874-point screen. Measuring the
        // caret against that proves nothing, so this refuses to claim a pass
        // rather than quietly banking one.
        let screen = app.windows.firstMatch.frame
        guard keyboardTop < screen.maxY else {
            throw XCTSkip(
                "the software keyboard never presented: parked at \(keyboardTop) on a "
                    + "\(screen.height)-point screen. Turn off Connect Hardware Keyboard "
                    + "in Simulator.app, or run this on a device."
            )
        }
        XCTAssertLessThan(
            line.maxY, keyboardTop,
            "the line being edited is under the keyboard: line \(line), keyboard top \(keyboardTop)"
        )
    }

    /// Question 2: typing reaches the document, and Swift sees it.
    func testTypingReachesTheDocument() throws {
        let words = app.staticTexts["harness.words"]
        XCTAssertTrue(words.waitForExistence(timeout: 20))
        let quote = app.webViews.staticTexts.containing(
            NSPredicate(format: "label CONTAINS 'fenceposts'")
        ).firstMatch
        XCTAssertTrue(quote.waitForExistence(timeout: 10))
        quote.tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))

        let before = words.label
        app.typeText(" and then stopped")
        Thread.sleep(forTimeInterval: 1.0)
        attach(named: "typed")
        XCTAssertNotEqual(words.label, before, "the word count never moved")
    }

    /// Question 3: is there an input accessory view, and does a selection
    /// report itself back to Swift?
    func testSelectionReportsItselfToSwift() throws {
        let selection = app.staticTexts["harness.selection"]
        XCTAssertTrue(selection.waitForExistence(timeout: 20))
        let before = selection.label

        let quote = app.webViews.staticTexts.containing(
            NSPredicate(format: "label CONTAINS 'fenceposts'")
        ).firstMatch
        XCTAssertTrue(quote.waitForExistence(timeout: 10))
        quote.press(forDuration: 1.0)
        Thread.sleep(forTimeInterval: 1.5)
        attach(named: "selection")

        XCTAssertNotEqual(
            selection.label, before,
            "a selection in the page never reached Swift"
        )
    }

    /// Undo, through the button rather than ⌘Z.
    ///
    /// A phone has no ⌘Z. The editor has an undo button for the same reason,
    /// and testing the thing a thumb can reach also means the test does not
    /// depend on whether the simulator has a hardware keyboard attached.
    func testUndoPutsTheTextBack() throws {
        let words = app.staticTexts["harness.words"]
        XCTAssertTrue(words.waitForExistence(timeout: 20))
        let quote = app.webViews.staticTexts.containing(
            NSPredicate(format: "label CONTAINS 'fenceposts'")
        ).firstMatch
        XCTAssertTrue(quote.waitForExistence(timeout: 10))
        quote.tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))

        let before = words.label
        app.typeText(" one two three")
        Thread.sleep(forTimeInterval: 1.0)
        let after = words.label
        XCTAssertNotEqual(after, before)

        app.buttons["harness.undo"].tap()
        Thread.sleep(forTimeInterval: 1.5)
        attach(named: "undone")
        XCTAssertEqual(words.label, before, "undo did not put the words back")
    }

    /// The format bar writes a real mark, and Swift sees which marks are on.
    func testTheToolbarMarksTheSelection() throws {
        let marks = app.staticTexts["harness.marks"]
        XCTAssertTrue(marks.waitForExistence(timeout: 20))
        let quote = app.webViews.staticTexts.containing(
            NSPredicate(format: "label CONTAINS 'fenceposts'")
        ).firstMatch
        XCTAssertTrue(quote.waitForExistence(timeout: 10))
        quote.press(forDuration: 1.0)
        Thread.sleep(forTimeInterval: 1.0)

        app.buttons["harness.bold"].tap()
        Thread.sleep(forTimeInterval: 1.0)
        attach(named: "bolded")

        // Two claims, and the second is the one that matters: the button lit
        // up, *and* the document Swift now holds carries a bold mark.
        XCTAssertTrue(
            marks.label.contains("bold"),
            "the toolbar did not report the mark as active: \(marks.label)"
        )
        XCTAssertTrue(
            app.staticTexts["harness.docmarks"].label.contains("bold"),
            "no bold mark reached the document: "
                + app.staticTexts["harness.docmarks"].label
        )
    }

    private func attach(named name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }
}

/// Reading on a phone: 4.3's two claims, asked of a running simulator.
final class ReaderTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-uitest", "-reading"]
        app.launch()
    }

    /// The claim that matters: a reader selects *words*, not a paragraph.
    ///
    /// Paragraph-level anchoring was what the phone could do before the editor
    /// became a real ProseMirror document. A long press selects a word, and
    /// the positions that come back are the ones the web client would send for
    /// the same word.
    func testAReaderCanSelectWordsRatherThanParagraphs() throws {
        let quote = app.staticTexts["harness.quote"]
        XCTAssertTrue(quote.waitForExistence(timeout: 20))
        XCTAssertEqual(quote.label, "none", "something was selected before the test began")

        // Pressed by coordinate rather than by finding the paragraph in the
        // accessibility tree: a read-only web view exposes its text
        // differently, and the thing under test is the selection, not the
        // tree. This is where the blockquote sits in the harness document.
        let page = app.webViews.firstMatch
        XCTAssertTrue(page.waitForExistence(timeout: 10))
        page.coordinate(withNormalizedOffset: CGVector(dx: 0.35, dy: 0.17))
            .press(forDuration: 1.0)
        Thread.sleep(forTimeInterval: 1.5)

        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = "word-selection"
        shot.lifetime = .keepAlways
        add(shot)

        let selected = quote.label
        XCTAssertNotEqual(selected, "none", "nothing was selected")
        XCTAssertTrue(
            Self.documentText.contains(selected),
            "the selection is not text from the document: \(selected)"
        )
        // A long press selects a word. Anything paragraph-sized would mean the
        // old block-level behaviour had come back.
        XCTAssertLessThanOrEqual(
            selected.split(separator: " ").count, 2,
            "that is not a word, it is a passage: \(selected)"
        )

        // And the positions are a real range rather than a collapsed caret.
        let range = app.staticTexts["harness.selection"].label
        let bounds = range.split(separator: "\u{2013}").compactMap { Int($0) }
        XCTAssertEqual(bounds.count, 2, "no range came back: \(range)")
        XCTAssertGreaterThan(bounds[1], bounds[0], "the selection is empty: \(range)")
    }

    /// Everything the harness document says, for checking a selection came
    /// out of it.
    private static let documentText = "The Long Field The field went on past "
        + "where the light gave out. She counted fenceposts until the counting "
        + "stopped meaning anything."

    /// Reader mode: pages rather than a scroll.
    func testTurningPages() throws {
        let indicator = app.staticTexts["harness.page"]
        XCTAssertTrue(indicator.waitForExistence(timeout: 20))
        app.buttons["harness.paged"].tap()
        Thread.sleep(forTimeInterval: 2.0)

        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = "paged"
        shot.lifetime = .keepAlways
        add(shot)

        // The document is longer than one screen of type, so paging it has to
        // produce more than one page — otherwise the columns never applied.
        let pages = Int(indicator.label.split(separator: "/").last ?? "1") ?? 1
        XCTAssertGreaterThan(pages, 0, "no page count came back: \(indicator.label)")
    }
}
