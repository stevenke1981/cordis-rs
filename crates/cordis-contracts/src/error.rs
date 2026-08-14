use thiserror::Error;

/// Validation failures are stable enough for CLI and MCP clients to surface.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ContractError {
    #[error("{field} is required")]
    Missing { field: &'static str },
    #[error("{field} must be non-empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds the maximum length of {max}")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains duplicate value: {value}")]
    Duplicate { field: &'static str, value: String },
    #[error("{field} contains unsupported value: {value}")]
    Unsupported { field: &'static str, value: String },
    #[error("{field} references unknown value: {value}")]
    UnknownReference { field: &'static str, value: String },
    #[error("{field} may not overlap with {other}: {value}")]
    Overlap {
        field: &'static str,
        other: &'static str,
        value: String,
    },
    #[error("{field} must be a subset of {other}: {value}")]
    NotSubset {
        field: &'static str,
        other: &'static str,
        value: String,
    },
    #[error("{field} contains a dependency cycle")]
    Cycle { field: &'static str },
    #[error("{field} is internally inconsistent: {reason}")]
    Inconsistent { field: &'static str, reason: String },
}

pub type ContractResult<T> = Result<T, ContractError>;

pub(crate) fn validate_text(value: &str, field: &'static str, max: usize) -> ContractResult<()> {
    if value.trim().is_empty() {
        return Err(ContractError::Empty { field });
    }
    if value.chars().count() > max {
        return Err(ContractError::TooLong { field, max });
    }
    Ok(())
}

pub(crate) fn validate_texts(
    values: &[String],
    field: &'static str,
    max_items: usize,
    max_len: usize,
) -> ContractResult<()> {
    if values.len() > max_items {
        return Err(ContractError::Inconsistent {
            field,
            reason: format!("at most {max_items} entries are allowed"),
        });
    }
    for value in values {
        validate_text(value, field, max_len)?;
    }
    Ok(())
}
