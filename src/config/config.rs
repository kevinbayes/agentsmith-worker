use std::{fs, io};
use std::collections::HashMap;
use std::io::Read;
use serde::Deserialize;

pub fn read_json_config(file_path: &str) -> Result<Config, io::Error> {

    let mut file = fs::File::open(file_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let config: Config = serde_json::from_str(&contents)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(config)
}

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub project: Option<ProjectConfig>,
    pub host: Option<HostConfig>,
    pub security: Option<SecurityConfig>,
}


#[derive(Clone, Debug, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    #[serde(rename = "sandbox-root")]
    pub sandboxroot: String,
}


#[derive(Clone, Debug, Deserialize)]
pub struct SecurityConfig {
    // Define your configuration structure
    #[serde(rename = "oauth")]
    pub oauth: OAuthConfig,
    // Define your configuration structure
    #[serde(rename = "jwt")]
    pub jwt: SecurityJwtConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OAuthConfig {
    // Define your configuration structure
    #[serde(rename = "jwks_domain")]
    pub jwks_domain: String,
    #[serde(rename = "jwks_protocol")]
    pub jwks_protocol: String,
    #[serde(rename = "jwks_path")]
    pub jwks_path: String,
    #[serde(rename = "audience")]
    pub audience: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SecurityJwtConfig {
    // Define your configuration structure
    #[serde(rename = "secret")]
    pub secret: String,
    #[serde(rename = "issuer")]
    pub issuer: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HostConfig {
    // Define your configuration structure
    pub host: String,
    pub port: i32,
}