use color_eyre::eyre::{eyre, Context, ContextCompat};
use serde_json::Value;

use super::util::paper_http_client;
use crate::error::Error;

/// Flatten a Fill v3 project document. Groups and versions are newest-first.
pub fn versions_from_fill_project(response: &Value) -> Result<Vec<String>, Error> {
    let groups = response
        .get("versions")
        .context("Failed to get paper versions, response does not contain versions")?
        .as_object()
        .context("Failed to get paper versions, versions is not an object")?;
    let mut versions = Vec::new();
    for (_group, list) in groups {
        let list = list
            .as_array()
            .ok_or_else(|| eyre!("Failed to get paper versions. Version group is not an array"))?;
        for version in list {
            let version = version.as_str().ok_or_else(|| {
                eyre!("Failed to get paper versions. Version string is not a string")
            })?;
            versions.push(version.to_string());
        }
    }
    Ok(versions)
}

pub async fn get_paper_minecraft_versions() -> Result<Vec<String>, Error> {
    let http = paper_http_client();
    let response: Value = http
        .get("https://fill.papermc.io/v3/projects/paper")
        .send()
        .await
        .context("Failed to get paper versions")?
        .error_for_status()
        .context("Failed to get paper versions")?
        .json()
        .await
        .context("Failed to get paper versions, response is not valid json")?;
    versions_from_fill_project(&response)
}

#[cfg(test)]
mod test {
    #[test]
    fn test_versions_from_fill_project() {
        let body = serde_json::json!({
            "versions": {
                "1.21": ["1.21.1", "1.21"],
                "1.20": ["1.20.4"]
            }
        });
        let versions = super::versions_from_fill_project(&body).unwrap();
        assert_eq!(versions, vec!["1.21.1", "1.21", "1.20.4"]);
    }
}
