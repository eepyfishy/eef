//! Backend request serialization for the interpreter's generation schema.
//! Proposal values and the independently validated contract retain their shape.
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::{Map, Value};

const PROPOSAL_FIELDS: [&str; 10] = [
    "schema_version",
    "goal",
    "intent",
    "context_hints",
    "constraints",
    "suggested_capabilities",
    "complexity",
    "workload",
    "needs_clarification",
    "clarification",
];

pub(crate) struct Payload<'a>(pub &'a Value);

impl Serialize for Payload<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some(fields) = self.0.as_object() else {
            return self.0.serialize(serializer);
        };
        let mut map = serializer.serialize_map(Some(fields.len()))?;
        for (key, value) in fields {
            if matches!(key.as_str(), "format" | "response_format") {
                map.serialize_entry(key, &Schema(value))?;
            } else {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

struct Schema<'a>(&'a Value);

impl Serialize for Schema<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    let proposal = value.as_object().filter(|properties| {
                        key == "properties"
                            && properties.len() == PROPOSAL_FIELDS.len()
                            && PROPOSAL_FIELDS
                                .iter()
                                .all(|field| properties.contains_key(*field))
                    });
                    if let Some(properties) = proposal {
                        map.serialize_entry(key, &ProposalProperties(properties))?;
                    } else {
                        map.serialize_entry(key, &Schema(value))?;
                    }
                }
                map.end()
            }
            Value::Array(values) => {
                let mut sequence = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    sequence.serialize_element(&Schema(value))?;
                }
                sequence.end()
            }
            _ => self.0.serialize(serializer),
        }
    }
}

struct ProposalProperties<'a>(&'a Map<String, Value>);

impl Serialize for ProposalProperties<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(PROPOSAL_FIELDS.len()))?;
        for field in PROPOSAL_FIELDS {
            map.serialize_entry(field, &Schema(&self.0[field]))?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn both_backend_schema_locations_put_goal_before_clarification_without_value_changes() {
        let schema = crate::interpretation::output_schema(&["system.info".into()]).unwrap();
        for payload in [
            json!({"messages":[{"role":"user","content":"Is this node working?"}],"format":schema}),
            json!({"response_format":{"type":"json_object","schema":schema},"max_tokens":512}),
        ] {
            let text = serde_json::to_string(&Payload(&payload)).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), payload);
            assert_eq!(
                text.matches("\"properties\":{\"schema_version\":").count(),
                2
            );
            let goal = text.find("\"goal\":").unwrap();
            let decision = text.find("\"needs_clarification\":").unwrap();
            let question = text.find("\"clarification\":").unwrap();
            assert!(goal < decision && decision < question);
        }
    }

    #[test]
    fn unrelated_schemas_and_message_objects_keep_their_existing_serialization() {
        let proposal = crate::interpretation::output_schema(&[]).unwrap();
        for payload in [
            json!({"messages":[{"role":"user","content":proposal}],"options":{"temperature":0}}),
            json!({"format":{"type":"object","properties":{"goal":{"type":"string"},"other":{"type":"boolean"}}}}),
            json!({"response_format":{"schema":{"type":"string"}}}),
        ] {
            assert_eq!(
                serde_json::to_string(&Payload(&payload)).unwrap(),
                serde_json::to_string(&payload).unwrap()
            );
        }
    }
}
