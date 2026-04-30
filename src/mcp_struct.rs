use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct PromptDoit {
    #[schemars(description = "Instructions for CodePal to execute")]
    pub instructions: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct PromptSecAudit {
    #[schemars(description = "What to perform the security audit on")]
    pub what: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct LsDirParams {
    #[schemars(description = "Path of directory to list")]
    pub path: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct LsDirResult {
    #[schemars(description = "Directory listing")]
    pub entries: Vec<String>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct ReadFileParams {
    #[schemars(description = "Path of file to read")]
    pub path: String,
    #[schemars(description = "Optional: No line before, 1-based inclusive")]
    pub start_line: Option<u32>,
    #[schemars(description = "Optional: No line after, 1-based inclusive")]
    pub end_line: Option<u32>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct GrepFileParams {
    #[schemars(description = "Path of file to search")]
    pub path: String,
    #[schemars(description = "Regex pattern to search for")]
    pub pattern: String,
    #[schemars(description = "Optional: Case insensitive (default: false)")]
    pub case_insensitive: Option<bool>,
    #[schemars(description = "Optional: Enable `.` matches `\\n` (default: false)")]
    pub dot_matches_newline: Option<bool>,
    #[schemars(description = "Optional: Context lines, like grep -C (default: 0)")]
    pub context_lines: Option<u16>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct MemoryStoreParams {
    #[schemars(description = "Keys to store the value under (all keys map to the same value)")]
    pub keys: Vec<String>,
    #[schemars(description = "Value to store")]
    pub value: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct MemoryStoreResult {
    #[schemars(description = "Whether the value was stored successfully")]
    pub success: bool,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct MemoryLoadParams {
    #[schemars(description = "Keys to look up; first key found in the store is returned")]
    pub keys: Vec<String>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct MemoryLoadResult {
    #[schemars(description = "The stored value, or null if not found")]
    pub value: Option<String>,
}
