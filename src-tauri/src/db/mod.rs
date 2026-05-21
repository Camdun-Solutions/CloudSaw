// Local SQLite storage. Owns the migration runner, app-data path helpers, and
// any low-level DB plumbing. Credentials are NEVER stored in SQLite — keychain
// only. See CLAUDE.md §4.3.

pub mod migrations;
pub mod paths;
