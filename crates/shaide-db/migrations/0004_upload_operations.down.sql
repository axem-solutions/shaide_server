-- Explicit downgrade drops only operation fences; settle remote work first.
DROP TABLE file_operations;
DROP TABLE collection_operations;
