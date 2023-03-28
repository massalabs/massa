mod error;
mod settings;

pub use error::VersioningMiddlewareError;
pub use settings::VersioningConfig;

#[cfg(test)]
pub mod tests;
