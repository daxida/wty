use crate::{Map, models::kaikki::Tag};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub enum YomitanEntry {
    TermBank(TermBank),
    TermBankMeta(TermBankMeta),
}

// https://github.com/yomidevs/yomitan/blob/f271fc0da3e55a98fa91c9834d75fccc96deae27/ext/data/schemas/dictionary-term-bank-v3-schema.json
//
// https://github.com/MarvNC/yomichan-dict-builder/blob/master/src/types/yomitan/termbank.ts
// @ TermInformation
#[derive(Debug, Serialize, Clone)]
pub struct TermBank(
    pub String,                  // term
    pub String,                  // reading
    pub String,                  // definition_tags
    pub String,                  // rules
    pub u32,                     // frequency
    pub Vec<DetailedDefinition>, // definitions
    pub u32,                     // sequence
    pub String,                  // term_tags
);

// https://github.com/yomidevs/yomitan/blob/f271fc0da3e55a98fa91c9834d75fccc96deae27/ext/data/schemas/dictionary-term-meta-bank-v3-schema.json
//
// https://github.com/MarvNC/yomichan-dict-builder/blob/master/src/types/yomitan/termbankmeta.ts
#[derive(Debug, Serialize, Clone)]
pub struct TermBankMeta(
    pub String,                // term
    pub String,                // static: "ipa"
    pub PhoneticTranscription, // phonetic transcription
);

#[derive(Debug, Serialize, Clone, PartialEq, Eq, Hash)]
pub struct PhoneticTranscription {
    pub reading: String,
    pub transcriptions: Vec<Ipa>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct Ipa {
    pub ipa: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
}

// https://github.com/MarvNC/yomichan-dict-builder/blob/master/src/types/yomitan/termbank.ts
// @ StructuredContentNode
#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub enum Node {
    Text(String),              // 32
    Array(Vec<Node>),          // 32
    Generic(Box<GenericNode>), // 16
    Backlink(BacklinkContent), // 40
}

impl Node {
    /// Push a new node into the array variant.
    pub fn push(&mut self, node: Self) {
        match self {
            Self::Array(boxed_vec) => boxed_vec.push(node),
            _ => panic!("Error: called 'push' with a non Node::Array"),
        }
    }

    pub const fn new_array() -> Self {
        Self::Array(vec![])
    }

    #[must_use]
    pub fn into_array_node(self) -> Self {
        Self::Array(vec![self])
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct NodeData(Map<String, String>);

impl<K, V> FromIterator<(K, V)> for NodeData
where
    K: Into<String>,
    V: Into<String>,
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let inner = iter
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        Self(inner)
    }
}

#[derive(Debug, Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum NTag {
    Span,
    Div,
    Ol,
    Ul,
    Li,
    Details,
    Summary,
}

// The order follows kty serialization, not yomichan builder order
#[derive(Debug, Serialize, Clone)]
pub struct GenericNode {
    pub tag: NTag,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<NodeData>,

    pub content: Node,
}

impl GenericNode {
    pub fn into_node(self) -> Node {
        Node::Generic(Box::new(self))
    }
}

#[derive(Debug, Clone)]
pub struct BacklinkContent {
    href: String,
    content: &'static str,
}

impl BacklinkContent {
    pub fn new(href: &str, content: &'static str) -> Self {
        Self {
            href: href.to_string(),
            content,
        }
    }
}

// Custom Serialize to not have to store the constant 'a' tag
impl Serialize for BacklinkContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("BacklinkContent", 3)?;
        state.serialize_field("tag", "a")?;
        state.serialize_field("href", &self.href)?;
        state.serialize_field("content", &self.content)?;
        state.end()
    }
}

// https://github.com/MarvNC/yomichan-dict-builder/blob/master/src/types/yomitan/termbank.ts
// @ DetailedDefinition
#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub enum DetailedDefinition {
    Text(String),
    StructuredContent(StructuredContent),
    Inflection((String, Vec<String>)),
}

impl DetailedDefinition {
    pub fn structured(content: Node) -> Self {
        Self::StructuredContent(StructuredContent {
            ty: "structured-content".to_string(),
            content,
        })
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct StructuredContent {
    #[serde(rename = "type")]
    ty: String, // should be hardcoded to "structured-content" (but then to serialize it...)
    content: Node,
}

pub fn wrap(tag: NTag, content_ty: &str, content: Node) -> Node {
    GenericNode {
        tag,
        title: None, // hardcoded since most of the wrap calls don't use it
        data: match content_ty {
            "" => None,
            _ => Some(NodeData::from_iter([("content", content_ty)])),
        },
        content,
    }
    .into_node()
}
