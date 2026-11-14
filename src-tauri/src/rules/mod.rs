//! Pure rule engine: models, condition & group evaluators, templates, validation,
//! rule evaluation pipeline, presets, and persistence service.
//!
//! Evaluation and template rendering perform zero filesystem or database I/O
//! (docs/ARCHITECTURE.md §1, §4).

pub mod conditions;
pub mod evaluator;
pub mod models;
pub mod presets;
pub mod service;
pub mod templates;
pub mod validate;

pub use conditions::*;
pub use evaluator::*;
pub use models::*;
pub use presets::*;
pub use service::*;
pub use templates::*;
pub use validate::*;
