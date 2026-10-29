CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    model_provider TEXT NOT NULL,
    title TEXT
);
INSERT INTO threads (id, model_provider, title) VALUES
    ('t1-aihubmix', 'aihubmix', 'migrated'),
    ('t2-ccswitch', 'ccswitch', 'migrated (legacy id)'),
    ('t3-openai', 'openai', 'official, untouched'),
    ('t4-private', 'my-private-relay', 'unknown id, untouched'),
    ('t5-custom', 'custom', 'already custom');
