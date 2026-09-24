-- SQLite before 3.35 cannot drop a column; the supported versions can.
ALTER TABLE publications DROP COLUMN spoiler_safe;
DROP TABLE blocked_readers;
DROP TABLE comment_reports;
