//! Correlated registration acknowledgement, not scheduling/execution authority.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub schema_version: u32,
    pub runtime_id: String,
    pub nonce: String,
}
impl Offer {
    pub fn new(runtime_id: &str) -> Result<Self> {
        let offer = Self {
            schema_version: 1,
            runtime_id: runtime_id.into(),
            nonce: uuid::Uuid::new_v4().to_string(),
        };
        offer.validate()?;
        Ok(offer)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || !crate::enrollment::runtime_valid(&self.runtime_id)
            || !crate::enrollment::runtime_valid(&self.nonce)
        {
            bail!("invalid registration receipt offer")
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    #[serde(rename = "type")]
    pub kind: String,
    pub schema_version: u32,
    pub node_id: String,
    pub runtime_id: String,
    pub nonce: String,
    pub session_id: String,
}
impl Receipt {
    pub fn new(node: &str, offer: &Offer, session: &str) -> Result<Self> {
        let receipt = Self {
            kind: "registration_accepted".into(),
            schema_version: 1,
            node_id: node.into(),
            runtime_id: offer.runtime_id.clone(),
            nonce: offer.nonce.clone(),
            session_id: session.into(),
        };
        receipt.validate(node, offer, session)?;
        Ok(receipt)
    }
    pub fn validate(&self, node: &str, offer: &Offer, session: &str) -> Result<()> {
        offer.validate()?;
        crate::network::validate_remote_node_id(node)?;
        if self.kind != "registration_accepted"
            || self.schema_version != 1
            || self.node_id != node
            || self.runtime_id != offer.runtime_id
            || self.nonce != offer.nonce
            || self.session_id != session
            || !crate::enrollment::runtime_valid(session)
        {
            bail!("registration receipt context mismatch")
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    NotConnected,
    Unknown,
    AwaitingReceipt,
    Accepted,
}
impl State {
    pub fn observe(
        live: &Value,
        node: &str,
        channel: crate::submission::InputChannelState,
    ) -> Self {
        if live.pointer("/connection/state").and_then(Value::as_str) != Some("connected") {
            return Self::NotConnected;
        }
        if channel == crate::submission::InputChannelState::Unavailable {
            return Self::Unknown;
        }
        let record = &live["registration_observation"];
        let Ok(offer) = serde_json::from_value::<Offer>(record["offer"].clone()) else {
            return Self::Unknown;
        };
        if offer.validate().is_err() || live["runtime_id"] != offer.runtime_id {
            return Self::Unknown;
        }
        let Some(session) = record["auth_session_id"]
            .as_str()
            .filter(|s| crate::enrollment::runtime_valid(s))
        else {
            return Self::Unknown;
        };
        if record["receipt"].is_null() {
            return Self::AwaitingReceipt;
        }
        let Ok(receipt) = serde_json::from_value::<Receipt>(record["receipt"].clone()) else {
            return Self::Unknown;
        };
        if receipt.validate(node, &offer, session).is_ok() {
            Self::Accepted
        } else {
            Self::Unknown
        }
    }
}
pub(crate) fn pending(offer: &Offer, session: Option<&str>) -> Value {
    json!({"offer":offer,"auth_session_id":session,"receipt":null})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipts_require_exact_fresh_context_and_are_not_historical_readiness() {
        let runtime = uuid::Uuid::new_v4().to_string();
        let offer = Offer::new(&runtime).unwrap();
        let session = uuid::Uuid::new_v4().to_string();
        let receipt = Receipt::new("stable", &offer, &session).unwrap();
        let mut live = json!({"runtime_id":runtime,"connection":{"state":"connected"},"registration_observation":pending(&offer,Some(&session))});
        let ready = crate::submission::InputChannelState::Ready;
        assert_eq!(
            State::observe(&live, "stable", ready),
            State::AwaitingReceipt
        );
        live["registration_observation"]["receipt"] = json!(receipt);
        assert_eq!(State::observe(&live, "stable", ready), State::Accepted);
        assert_eq!(
            State::observe(
                &live,
                "stable",
                crate::submission::InputChannelState::Unavailable
            ),
            State::Unknown
        );
        for (field, value) in [
            ("node_id", json!("other")),
            ("runtime_id", json!(uuid::Uuid::new_v4().to_string())),
            ("nonce", json!(uuid::Uuid::new_v4().to_string())),
            ("session_id", json!(uuid::Uuid::new_v4().to_string())),
            ("execution_authorized", json!(true)),
        ] {
            let mut wrong = live.clone();
            wrong["registration_observation"]["receipt"][field] = value;
            assert_eq!(
                State::observe(&wrong, "stable", ready),
                State::Unknown,
                "{field}"
            );
        }
        let mut retired = live.clone();
        retired["runtime_id"] = json!(uuid::Uuid::new_v4().to_string());
        assert_eq!(State::observe(&retired, "stable", ready), State::Unknown);
        live["connection"]["state"] = json!("retrying");
        assert_eq!(State::observe(&live, "stable", ready), State::NotConnected);
        let new_offer = Offer::new(&runtime).unwrap();
        assert_ne!(new_offer.nonce, offer.nonce);
        assert!(receipt.validate("stable", &new_offer, &session).is_err());
    }
}
