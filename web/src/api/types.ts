/**
 * The app's view of the API contract.
 *
 * Every shape here comes from `schema.ts`, which `cargo xtask codegen`
 * generates from the Rust handlers. Nothing in this file redeclares a field —
 * it only renames a few types to what the app calls them and adds the helpers
 * that are behaviour rather than shape. Rename a field in the backend and this
 * file's consumers stop compiling, which is the whole point.
 */
export type {
  Anchor,
  AuthorCard,
  BibleEntry,
  BibleKind,
  BlockChangeKind,
  BlockDiff,
  CircleMember,
  CircleView,
  InviteView,
  PublicationInvite,
  ReaderProgressView,
  ReadingProgress,
  ReactionKind,
  ReactionCount,
  BlockReactions,
  ReviewQuestion,
  ReviewAnswer,
  AnswerView,
  QuestionKind,
  QuestionScope,
  Standing,
  SinceResponse,
  NoteFate,
  NoteStatus,
  NoteCounts,
  BulkStateResponse,
  ReportReason,
  ReportView,
  BlockView,
  BlockedReader,
  TaskView,
  TaskSource,
  TaskState,
  RevisionTask,
  CommentReport,
  Chapter,
  Comment,
  CommentCategory,
  CommentState,
  CommentView,
  DiffResponse,
  DiffSummary,
  DocNode,
  DocMark,
  ErrorBody,
  ErrorEnvelope,
  JsonValue,
  Op,
  OutlineChapter,
  Publication,
  PublicationCard,
  PublicationScope,
  RevisionWithContent,
  SaveResult,
  Scene,
  SceneChangeKind,
  SceneDiff,
  SceneRevision,
  SceneStatus,
  SceneWithContent,
  TextChange,
  TrackedChange,
  User,
  Version,
  VersionContent,
  VersionDetail,
  VersionEntry,
  Visibility,
  WorkKind,
  WorkStatus,
  BulkResolveResponse,
  EntryView,
  ResolveResponse,
  RestoreResponse,
  VersionSummary,
  SplitSceneResponse,
  StaleChange,
  Work,
  WritingDay,
  Profile,
  ReviewHistoryItem,
  FollowState,
  Genre,
  LengthBand,
  WindowFilter,
  FeedSort,
  WritingSummary,
} from './schema'

import type { Anchor, OutlineResponse, SessionResponse, TemplateView } from './schema'

/** Names the app uses for a few server-side types. */
export type Session = SessionResponse
export type Template = TemplateView
export type Outline = OutlineResponse

/** `Comment.anchor` is stored as a JSON string; this is the parsed form. */
export function parseAnchor(raw: string | null | undefined): Anchor | null {
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as Anchor
    return typeof parsed.from === 'number' && typeof parsed.to === 'number' ? parsed : null
  } catch {
    return null
  }
}
