// Scanner role connection — Phase 2 replacement for the deleted
// `terraform::*` provisioning module.
//
// CloudSaw no longer creates the scanner role itself. The user creates
// it via whichever IaC posture matches their environment (Console,
// Terraform, CloudFormation, or AWS CLI — all four recipes are
// rendered with pre-substituted values in the onboarding form), and
// `scanner_role::connect()` validates + records it.
//
// Public surface (consumed by `ipc::*`):
//   * `requirements(aws_account_id)` — values the UI renders into the
//     four recipe blocks (caller ARN + external_id).
//   * `connect(aws_account_id, role_arn, policy_variant)` — validates
//     the user's role via a dry-run `sts:AssumeRole` and persists it.
//   * `status(aws_account_id)` — read-side variant of the old
//     `terraform::provisioning_status`; reads the same SQLite columns
//     migration 0004 already wrote.

pub mod connect;
pub mod error;
pub mod storage;
pub mod types;

pub use connect::{connect, requirements, status};
pub use error::ScannerRoleError;
pub use types::{ConnectResult, PolicyVariant, ProvisioningStatus, RoleRequirements};
