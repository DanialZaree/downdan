CREATE TABLE IF NOT EXISTS download_engines (
    download_id INTEGER NOT NULL PRIMARY KEY REFERENCES downloads(id) ON DELETE CASCADE,
    engine TEXT NOT NULL DEFAULT 'native',
    gid TEXT
);

