//! Owner interpreter sampling preferences; no model-name presets or execution.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// Integer thousandths keep policy equality/digests exact. CLI accepts decimals.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InterpreterSampling {
    pub temperature_milli: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p_milli: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_p_milli: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty_milli: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_penalty_milli: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
}
impl InterpreterSampling {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn validate(&self) -> Result<()> {
        if self.temperature_milli > 2000
            || self.top_p_milli.is_some_and(|v| !(1..=1000).contains(&v))
            || self.top_k.is_some_and(|v| v > 1024)
            || self.min_p_milli.is_some_and(|v| v > 1000)
            || self
                .presence_penalty_milli
                .is_some_and(|v| !(-2000..=2000).contains(&v))
            || self
                .repeat_penalty_milli
                .is_some_and(|v| !(100..=2000).contains(&v))
            || self.seed == Some(u32::MAX)
        {
            bail!("Invalid interpreter sampling bounds")
        }
        Ok(())
    }
    /// Applied only to internally generated interpretation/preparation requests.
    /// No token/deadline/permission/selection fields are produced here.
    pub(crate) fn options(&self) -> Value {
        let mut options = json!({"temperature":f64::from(self.temperature_milli)/1000.0});
        for (name, value) in [
            ("top_p", self.top_p_milli.map(|v| f64::from(v) / 1000.0)),
            ("min_p", self.min_p_milli.map(|v| f64::from(v) / 1000.0)),
            (
                "presence_penalty",
                self.presence_penalty_milli.map(|v| f64::from(v) / 1000.0),
            ),
            (
                "repeat_penalty",
                self.repeat_penalty_milli.map(|v| f64::from(v) / 1000.0),
            ),
        ] {
            if let Some(value) = value {
                options[name] = json!(value);
            }
        }
        if let Some(value) = self.top_k {
            options["top_k"] = json!(value);
        }
        if let Some(value) = self.seed {
            options["seed"] = json!(value);
        }
        options
    }
}

fn decimal_milli(raw: &str) -> std::result::Result<i32, String> {
    if raw.len() > 32 {
        return Err("Use a finite decimal with at most three fractional places".into());
    }
    let value = raw
        .parse::<f64>()
        .map_err(|_| "Use a finite decimal".to_owned())?;
    let scaled = value * 1000.0;
    if !scaled.is_finite()
        || !(-2000.0..=2000.0).contains(&scaled)
        || (scaled - scaled.round()).abs() > 0.000001
    {
        return Err("Use a value from -2 to 2 with at most three fractional places".into());
    }
    Ok(scaled.round() as i32)
}
fn unsigned_decimal(raw: &str) -> std::result::Result<u16, String> {
    u16::try_from(decimal_milli(raw)?).map_err(|_| "Use a nonnegative decimal".into())
}
fn signed_decimal(raw: &str) -> std::result::Result<i16, String> {
    i16::try_from(decimal_milli(raw)?).map_err(|_| "Invalid decimal".into())
}
fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::Args)]
#[serde(default, deny_unknown_fields)]
pub struct SamplingPatch {
    #[arg(long="interpreter-temperature",value_parser=unsigned_decimal)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_milli: Option<u16>,
    #[arg(long="interpreter-top-p",value_parser=unsigned_decimal)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p_milli: Option<u16>,
    #[arg(long="interpreter-top-k",value_parser=clap::value_parser!(u16).range(0..=1024))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u16>,
    #[arg(long="interpreter-min-p",value_parser=unsigned_decimal)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_p_milli: Option<u16>,
    #[arg(long="interpreter-presence-penalty",allow_hyphen_values=true,value_parser=signed_decimal)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty_milli: Option<i16>,
    #[arg(long="interpreter-repeat-penalty",value_parser=unsigned_decimal)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_penalty_milli: Option<u16>,
    /// Seed hint; repeatable output is not guaranteed across backends/hardware.
    #[arg(long="interpreter-seed",value_parser=clap::value_parser!(u32).range(0..4294967295))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
    /// Restore temperature zero and omit all optional interpreter sampling hints.
    #[arg(long="interpreter-sampling-default",conflicts_with_all=["temperature_milli","top_p_milli","top_k","min_p_milli","presence_penalty_milli","repeat_penalty_milli","seed"])]
    #[serde(skip_serializing_if = "is_false")]
    pub reset: bool,
}
impl SamplingPatch {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
    pub fn validate(&self) -> Result<()> {
        let mut values = self.clone();
        values.reset = false;
        if self.reset && !values.is_empty() {
            bail!("Interpreter sampling reset conflicts with explicit choices")
        }
        self.apply_unchecked(&InterpreterSampling::default())
            .validate()
    }
    fn apply_unchecked(&self, before: &InterpreterSampling) -> InterpreterSampling {
        if self.reset {
            return InterpreterSampling::default();
        }
        let mut next = before.clone();
        if let Some(value) = self.temperature_milli {
            next.temperature_milli = value;
        }
        if let Some(value) = self.top_p_milli {
            next.top_p_milli = Some(value);
        }
        if let Some(value) = self.top_k {
            next.top_k = Some(value);
        }
        if let Some(value) = self.min_p_milli {
            next.min_p_milli = Some(value);
        }
        if let Some(value) = self.presence_penalty_milli {
            next.presence_penalty_milli = Some(value);
        }
        if let Some(value) = self.repeat_penalty_milli {
            next.repeat_penalty_milli = Some(value);
        }
        if let Some(value) = self.seed {
            next.seed = Some(value);
        }
        next
    }
    pub(crate) fn apply(&self, before: &InterpreterSampling) -> Result<InterpreterSampling> {
        self.validate()?;
        let next = self.apply_unchecked(before);
        next.validate()?;
        Ok(next)
    }
    pub(crate) fn matches(&self, saved: &InterpreterSampling) -> bool {
        self.validate().is_ok()
            && saved.validate().is_ok()
            && (!self.reset || saved.is_default())
            && self
                .temperature_milli
                .is_none_or(|v| saved.temperature_milli == v)
            && self
                .top_p_milli
                .is_none_or(|v| saved.top_p_milli == Some(v))
            && self.top_k.is_none_or(|v| saved.top_k == Some(v))
            && self
                .min_p_milli
                .is_none_or(|v| saved.min_p_milli == Some(v))
            && self
                .presence_penalty_milli
                .is_none_or(|v| saved.presence_penalty_milli == Some(v))
            && self
                .repeat_penalty_milli
                .is_none_or(|v| saved.repeat_penalty_milli == Some(v))
            && self.seed.is_none_or(|v| saved.seed == Some(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_bounds_are_exact_and_do_not_produce_authority() {
        let default = InterpreterSampling::default();
        default.validate().unwrap();
        assert_eq!(default.options(), json!({"temperature":0.0}));
        for raw in [
            json!({"temperature_milli":2001}),
            json!({"top_p_milli":0}),
            json!({"top_p_milli":1001}),
            json!({"top_k":1025}),
            json!({"min_p_milli":1001}),
            json!({"presence_penalty_milli":-2001}),
            json!({"repeat_penalty_milli":99}),
            json!({"seed":u32::MAX}),
        ] {
            assert!(
                serde_json::from_value::<InterpreterSampling>(raw)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        for raw in [
            json!(null),
            json!({"temperature_milli":1.5}),
            json!({"temperature_milli":"1000"}),
            json!({"max_tokens":8192}),
            json!({"execution_authorized":true}),
        ] {
            assert!(serde_json::from_value::<InterpreterSampling>(raw).is_err());
        }
    }
    #[test]
    fn patch_updates_preserve_unchosen_values_and_explicit_reset_conflicts() {
        let before = InterpreterSampling {
            temperature_milli: 600,
            seed: Some(42),
            ..Default::default()
        };
        let patch = SamplingPatch {
            presence_penalty_milli: Some(2000),
            ..Default::default()
        };
        let after = patch.apply(&before).unwrap();
        assert_eq!(after.temperature_milli, 600);
        assert_eq!(after.seed, Some(42));
        assert!(patch.matches(&after));
        assert!(!patch.matches(&before));
        let reset = SamplingPatch {
            reset: true,
            ..Default::default()
        };
        assert!(reset.apply(&after).unwrap().is_default());
        assert!(reset.matches(&InterpreterSampling::default()));
        assert!(!reset.matches(&after));
        assert!(
            SamplingPatch {
                reset: true,
                temperature_milli: Some(0),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(serde_json::from_value::<SamplingPatch>(json!({"temperature":1.0})).is_err());
    }
    #[test]
    fn human_decimal_cli_values_have_bounded_exact_precision() {
        for (raw, value) in [
            ("0", 0),
            ("0.001", 1),
            ("0.6", 600),
            ("1.00", 1000),
            ("2", 2000),
        ] {
            assert_eq!(unsigned_decimal(raw).unwrap(), value);
        }
        assert_eq!(signed_decimal("-1.5").unwrap(), -1500);
        for raw in ["NaN", "inf", "-inf", "0.1234", "3", "0.0001"] {
            assert!(decimal_milli(raw).is_err());
        }
        assert!(unsigned_decimal("-0.1").is_err());
    }
}
