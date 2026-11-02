use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Inch,
    Millimeter,
    /// Pounds per square foot (distributed load).
    Psf,
    Degree,
}

impl Unit {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Inch => "in",
            Self::Millimeter => "mm",
            Self::Psf => "psf",
            Self::Degree => "deg",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub label: String,
    pub value: f64,
    pub unit: Unit,
    pub min: f64,
    pub max: f64,
    /// Values must be integer multiples of `step` measured from `min`
    /// when `snap` is true; otherwise `step` is only a UI increment.
    pub step: f64,
    #[serde(default)]
    pub snap: bool,
    #[serde(default)]
    pub group: String,
}

impl Parameter {
    pub fn validate(&self, candidate: f64) -> crate::Result<f64> {
        if !candidate.is_finite() {
            return Err(crate::Error::Validation(format!(
                "{} must be a finite number",
                self.label
            )));
        }
        if candidate < self.min - 1e-9 || candidate > self.max + 1e-9 {
            return Err(crate::Error::Validation(format!(
                "{} must be between {} and {} {}",
                self.label,
                fmt_num(self.min),
                fmt_num(self.max),
                self.unit.suffix()
            )));
        }
        if self.snap && self.step > 0.0 {
            let steps = (candidate - self.min) / self.step;
            if (steps - steps.round()).abs() > 1e-6 {
                return Err(crate::Error::Validation(format!(
                    "{} must be a multiple of {} {} (nearest valid: {} or {})",
                    self.label,
                    fmt_num(self.step),
                    self.unit.suffix(),
                    fmt_num(self.min + steps.floor() * self.step),
                    fmt_num(self.min + steps.ceil() * self.step),
                )));
            }
        }
        Ok(candidate)
    }
}

pub(crate) fn fmt_num(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width() -> Parameter {
        Parameter {
            name: "width_in".into(),
            label: "Dock width".into(),
            value: 96.0,
            unit: Unit::Inch,
            min: 48.0,
            max: 240.0,
            step: 24.0,
            snap: true,
            group: String::new(),
        }
    }

    #[test]
    fn snapped_parameter_reports_nearest_valid_values() {
        let err = width().validate(130.0).unwrap_err().to_string();
        assert!(err.contains("multiple of 24"), "{err}");
        assert!(err.contains("120 or 144"), "{err}");
        assert_eq!(width().validate(144.0).unwrap(), 144.0);
    }

    #[test]
    fn rejects_out_of_range_and_non_finite() {
        assert!(width().validate(24.0).is_err());
        assert!(width().validate(f64::NAN).is_err());
        assert!(width().validate(264.0).is_err());
    }
}
