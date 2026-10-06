-- A week's target beside the day's, on the writer: a week is a kinder unit
-- for anyone who writes on three evenings rather than seven.
ALTER TABLE users ADD COLUMN weekly_word_goal INTEGER;

-- A target for the book itself, and optionally a date to reach it by. On the
-- work, not the writer: two books, two targets.
ALTER TABLE works ADD COLUMN target_words INTEGER;
ALTER TABLE works ADD COLUMN target_date TEXT;
