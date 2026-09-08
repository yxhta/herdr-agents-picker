use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde::Deserialize;

/// Failures from talking to the herdr CLI or decoding its output.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to run {bin}: {source}")]
    Spawn { bin: String, source: std::io::Error },
    #[error("herdr {command} failed: {stderr}")]
    Command { command: String, stderr: String },
    #[error("invalid {what} JSON: {source}")]
    Parse {
        what: &'static str,
        source: serde_json::Error,
    },
    #[error("failed to read Herdr config {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid Herdr config {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("invalid ui.agent_panel_sort in Herdr config {path}: {value}")]
    AgentPanelSort { path: PathBuf, value: String },
}

/// Agent ordering modes exposed by Herdr's built-in sidebar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentPanelSort {
    #[default]
    Spaces,
    Priority,
}

fn parse_agent_panel_sort(raw: &str, path: &Path) -> Result<AgentPanelSort, Error> {
    let config: toml::Value = raw.parse().map_err(|source| Error::ConfigParse {
        path: path.to_path_buf(),
        source,
    })?;
    let Some(value) = config.get("ui").and_then(|ui| ui.get("agent_panel_sort")) else {
        return Ok(AgentPanelSort::Spaces);
    };
    match value.as_str() {
        Some("spaces" | "workspaces") => Ok(AgentPanelSort::Spaces),
        Some("priority") => Ok(AgentPanelSort::Priority),
        _ => Err(Error::AgentPanelSort {
            path: path.to_path_buf(),
            value: value.to_string(),
        }),
    }
}

fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HERDR_CONFIG_PATH") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("herdr/config.toml");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config/herdr/config.toml");
    }
    std::env::temp_dir().join("herdr/config.toml")
}

/// One detected agent pane, as reported by `herdr agent list`.
// Field names mirror the herdr CLI JSON payload keys verbatim.
#[expect(clippy::struct_field_names)]
#[derive(Debug, Deserialize)]
pub struct Agent {
    pub agent: Option<String>,
    pub agent_status: Option<String>,
    pub cwd: Option<String>,
    pub name: Option<String>,
    pub pane_id: Option<String>,
    pub terminal_id: Option<String>,
    pub terminal_title_stripped: Option<String>,
    pub workspace_id: Option<String>,
    pub tab_id: Option<String>,
    #[serde(default)]
    pub focused: bool,
}

impl Agent {
    /// Focus/read/get target. `herdr agent {focus,get,read}` only resolve
    /// `pane_id` targets (`terminal_id` started returning `agent_not_found`
    /// on Herdr 0.7.5); `terminal_id` is kept as a fallback for older
    /// Herdr builds or agents Herdr reports without a `pane_id`.
    pub fn target(&self) -> Option<&str> {
        non_empty(self.pane_id.as_deref()).or_else(|| non_empty(self.terminal_id.as_deref()))
    }

    /// Everything `Client::focus_agent` needs, owned so it outlives the
    /// agent list the picker keeps refreshing.
    pub fn focus(&self) -> Option<Focus> {
        Some(Focus {
            workspace_id: non_empty(self.workspace_id.as_deref()).map(str::to_string),
            tab_id: non_empty(self.tab_id.as_deref()).map(str::to_string),
            target: self.target()?.to_string(),
        })
    }

    pub fn kind(&self) -> &str {
        non_empty(self.agent.as_deref()).unwrap_or("-")
    }

    pub fn status(&self) -> &str {
        non_empty(self.agent_status.as_deref()).unwrap_or("unknown")
    }

    /// User-assigned agent name plus the terminal title, when both exist.
    pub fn label(&self) -> String {
        let name = non_empty(self.name.as_deref());
        let title = non_empty(self.terminal_title_stripped.as_deref());
        match (name, title) {
            (Some(name), Some(title)) => format!("{name} · {title}"),
            (Some(name), None) => name.to_string(),
            (None, Some(title)) => title.to_string(),
            (None, None) => "-".to_string(),
        }
    }

    pub fn short_cwd(&self, home: Option<&str>) -> String {
        let Some(cwd) = non_empty(self.cwd.as_deref()) else {
            return "-".to_string();
        };
        match home.filter(|home| !home.is_empty()) {
            Some(home) if cwd == home => "~".to_string(),
            Some(home) => cwd
                .strip_prefix(home)
                .and_then(|rest| rest.strip_prefix('/'))
                .map_or_else(|| cwd.to_string(), |rest| format!("~/{rest}")),
            None => cwd.to_string(),
        }
    }
}

/// Where an agent lives, resolved once at pick time.
#[derive(Debug)]
pub struct Focus {
    pub workspace_id: Option<String>,
    pub tab_id: Option<String>,
    pub target: String,
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

#[derive(Deserialize)]
struct AgentListResponse {
    result: Option<AgentListResult>,
}

#[derive(Deserialize)]
struct AgentListResult {
    #[serde(default)]
    agents: Vec<Agent>,
}

pub fn parse_agent_list(raw: &str) -> Result<Vec<Agent>, Error> {
    let response: AgentListResponse = parse_json(raw, "agent list")?;
    Ok(response.result.map(|r| r.agents).unwrap_or_default())
}

#[derive(Deserialize)]
struct ReadResponse {
    result: Option<ReadResult>,
}

#[derive(Deserialize)]
struct ReadResult {
    read: Option<ReadPayload>,
}

#[derive(Deserialize)]
struct ReadPayload {
    text: Option<String>,
}

pub fn parse_read_text(raw: &str) -> Result<String, Error> {
    let response: ReadResponse = parse_json(raw, "read")?;
    Ok(response
        .result
        .and_then(|r| r.read)
        .and_then(|r| r.text)
        .unwrap_or_default())
}

fn parse_json<'a, T: Deserialize<'a>>(raw: &'a str, what: &'static str) -> Result<T, Error> {
    serde_json::from_str(raw).map_err(|source| Error::Parse { what, source })
}

#[derive(Deserialize)]
struct WorkspaceListResponse {
    result: Option<WorkspaceListResult>,
}

#[derive(Deserialize)]
struct WorkspaceListResult {
    #[serde(default)]
    workspaces: Vec<WorkspaceEntry>,
}

#[derive(Deserialize)]
struct WorkspaceEntry {
    workspace_id: String,
    label: String,
    #[serde(default)]
    tab_count: u32,
}

#[derive(Deserialize)]
struct TabListResponse {
    result: Option<TabListResult>,
}

#[derive(Deserialize)]
struct TabListResult {
    #[serde(default)]
    tabs: Vec<TabEntry>,
}

#[derive(Deserialize)]
struct TabEntry {
    tab_id: String,
    label: String,
    #[serde(default)]
    number: u32,
}

#[derive(Debug)]
struct WorkspaceInfo {
    label: String,
    tab_count: u32,
}

#[derive(Debug)]
struct TabInfo {
    label: String,
    /// True when the label is just the tab's own number ("1", "2", …), i.e.
    /// the user never renamed it. Precomputed so `location_for` stays
    /// allocation-free on the lookup path.
    auto_named: bool,
}

/// Workspace/tab labels, as shown in the built-in sidebar's "agents" panel:
/// workspace name, plus the tab name when the workspace has more than one
/// tab or the tab was given a custom (non-numeric) name.
#[derive(Debug, Default)]
pub struct WorkspaceIndex {
    workspaces: HashMap<String, WorkspaceInfo>,
    tabs: HashMap<String, TabInfo>,
}

impl WorkspaceIndex {
    fn parse(workspaces_raw: &str, tabs_raw: &str) -> Result<Self, Error> {
        let workspaces: WorkspaceListResponse = parse_json(workspaces_raw, "workspace list")?;
        let tabs: TabListResponse = parse_json(tabs_raw, "tab list")?;
        let workspaces = workspaces
            .result
            .map(|r| r.workspaces)
            .unwrap_or_default()
            .into_iter()
            .map(|w| {
                let info = WorkspaceInfo {
                    label: w.label,
                    tab_count: w.tab_count,
                };
                (w.workspace_id, info)
            })
            .collect();
        let tabs = tabs
            .result
            .map(|r| r.tabs)
            .unwrap_or_default()
            .into_iter()
            .map(|t| {
                let info = TabInfo {
                    auto_named: t.label == t.number.to_string(),
                    label: t.label,
                };
                (t.tab_id, info)
            })
            .collect();
        Ok(Self { workspaces, tabs })
    }

    /// Mirrors the built-in sidebar's default agent row: workspace name,
    /// plus " · tab name" when the tab is worth distinguishing.
    pub fn location_for(&self, agent: &Agent) -> String {
        let workspace = agent
            .workspace_id
            .as_deref()
            .and_then(|id| self.workspaces.get(id));
        let workspace_label = workspace.map_or("-", |w| w.label.as_str());

        let tab = agent.tab_id.as_deref().and_then(|id| self.tabs.get(id));
        let multi_tab = workspace.is_some_and(|w| w.tab_count > 1);
        match tab {
            Some(tab) if multi_tab || !tab.auto_named => {
                format!("{workspace_label} · {}", tab.label)
            }
            _ => workspace_label.to_string(),
        }
    }
}

/// Thin client over the herdr CLI (`HERDR_BIN_PATH`), the documented way for
/// plugins to talk back to the running herdr instance.
pub struct Client {
    bin: String,
}

impl Client {
    pub fn from_env() -> Self {
        let bin = std::env::var("HERDR_BIN_PATH")
            .ok()
            .filter(|bin| !bin.is_empty())
            .unwrap_or_else(|| "herdr".to_string());
        Self::new(bin)
    }

    pub fn new(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }

    pub fn list_agents(&self) -> Result<Vec<Agent>, Error> {
        parse_agent_list(&self.run(&["agent", "list"])?)
    }

    pub fn agent_panel_sort() -> Result<AgentPanelSort, Error> {
        let path = config_path();
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AgentPanelSort::Spaces);
            }
            Err(source) => return Err(Error::ConfigRead { path, source }),
        };
        parse_agent_panel_sort(&raw, &path)
    }

    pub fn workspace_index(&self) -> Result<WorkspaceIndex, Error> {
        let workspaces = self.run(&["workspace", "list"])?;
        let tabs = self.run(&["tab", "list"])?;
        WorkspaceIndex::parse(&workspaces, &tabs)
    }

    pub fn read_agent(&self, target: &str) -> Result<String, Error> {
        parse_read_text(&self.run(&[
            "agent", "read", target, "--source", "visible", "--format", "ansi",
        ])?)
    }

    /// Herdr 0.9.0 renders the UI in each client, and `agent focus` alone only
    /// moves the server-side pane focus — the viewing client stays where it
    /// is. Switching its workspace and tab first is what actually navigates.
    pub fn focus_agent(&self, focus: &Focus) -> Result<(), Error> {
        if let Some(workspace_id) = &focus.workspace_id {
            self.run(&["workspace", "focus", workspace_id])?;
        }
        if let Some(tab_id) = &focus.tab_id {
            self.run(&["tab", "focus", tab_id])?;
        }
        self.run(&["agent", "focus", &focus.target]).map(|_| ())
    }

    pub fn observe_agent(&self, target: &str, columns: u16, rows: u16) -> Result<Child, Error> {
        Command::new(&self.bin)
            .args(["terminal", "session", "observe", target, "--cols"])
            .arg(columns.to_string())
            .arg("--rows")
            .arg(rows.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| Error::Spawn {
                bin: self.bin.clone(),
                source,
            })
    }

    fn run(&self, args: &[&str]) -> Result<String, Error> {
        let output = Command::new(&self.bin)
            .args(args)
            .output()
            .map_err(|source| Error::Spawn {
                bin: self.bin.clone(),
                source,
            })?;
        if !output.status.success() {
            return Err(Error::Command {
                command: args.join(" "),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        // Reuse the stdout buffer when it is valid UTF-8 (the normal case);
        // `from_utf8_lossy(..).into_owned()` would always copy it.
        Ok(String::from_utf8(output.stdout)
            .unwrap_or_else(|invalid| String::from_utf8_lossy(invalid.as_bytes()).into_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"id":"cli:agent:list","result":{"agents":[
        {"agent":"claude","agent_status":"working","cwd":"/Users/me/ghq/dotfiles",
         "focused":false,"pane_id":"w2:p1","terminal_id":"term_abc",
         "terminal_title_stripped":"agents-picker plugin","workspace_id":"w2","tab_id":"w2:t1"},
        {"agent":"codex","agent_status":"idle","cwd":"/Users/me",
         "focused":true,"name":"impl-1.2","pane_id":"w7:p2","terminal_id":"",
         "terminal_title_stripped":"","workspace_id":"w7","tab_id":"w7:t1"}
    ],"type":"agent_list"}}"#;

    #[test]
    fn parses_agent_list() {
        let agents = parse_agent_list(SAMPLE).unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].kind(), "claude");
        assert_eq!(agents[0].status(), "working");
        assert!(agents[1].focused);
    }

    #[test]
    fn target_prefers_pane_id_and_falls_back_to_terminal_id() {
        let agents = parse_agent_list(SAMPLE).unwrap();
        assert_eq!(agents[0].target(), Some("w2:p1"));
        assert_eq!(agents[1].target(), Some("w7:p2"));
    }

    #[test]
    fn focus_carries_the_workspace_and_tab_the_client_must_switch_to() {
        let agents = parse_agent_list(SAMPLE).unwrap();
        let focus = agents[0].focus().unwrap();
        assert_eq!(focus.workspace_id.as_deref(), Some("w2"));
        assert_eq!(focus.tab_id.as_deref(), Some("w2:t1"));
        assert_eq!(focus.target, "w2:p1");
    }

    #[test]
    fn label_combines_name_and_title() {
        let agents = parse_agent_list(SAMPLE).unwrap();
        assert_eq!(agents[0].label(), "agents-picker plugin");
        assert_eq!(agents[1].label(), "impl-1.2");
    }

    #[test]
    fn short_cwd_replaces_home_prefix() {
        let agents = parse_agent_list(SAMPLE).unwrap();
        assert_eq!(agents[0].short_cwd(Some("/Users/me")), "~/ghq/dotfiles");
        assert_eq!(agents[1].short_cwd(Some("/Users/me")), "~");
        assert_eq!(agents[0].short_cwd(None), "/Users/me/ghq/dotfiles");
    }

    #[test]
    fn short_cwd_does_not_shorten_sibling_prefix() {
        let agents = parse_agent_list(SAMPLE).unwrap();
        // "/Users/me…" must not match home "/Users/m".
        assert_eq!(agents[1].short_cwd(Some("/Users/m")), "/Users/me");
    }

    #[test]
    fn parses_empty_and_read_payloads() {
        assert!(parse_agent_list(r#"{"result":{"agents":[]}}"#)
            .unwrap()
            .is_empty());
        assert!(parse_agent_list("not json").is_err());
        assert_eq!(
            parse_read_text(r#"{"result":{"read":{"text":"hello"}}}"#).unwrap(),
            "hello"
        );
        assert_eq!(parse_read_text(r#"{"result":{}}"#).unwrap(), "");
    }

    #[test]
    fn parse_error_names_the_payload() {
        let err = parse_agent_list("not json").unwrap_err();
        assert!(err.to_string().starts_with("invalid agent list JSON:"));
    }

    #[test]
    fn agent_panel_sort_parses_herdr_values_and_default() {
        let path = Path::new("config.toml");
        assert_eq!(
            parse_agent_panel_sort("[ui]\nagent_panel_sort = \"priority\"", path).unwrap(),
            AgentPanelSort::Priority
        );
        assert_eq!(
            parse_agent_panel_sort("[ui]\nagent_panel_sort = \"workspaces\"", path).unwrap(),
            AgentPanelSort::Spaces
        );
        assert_eq!(
            parse_agent_panel_sort("[theme]\nname = \"kanagawa\"", path).unwrap(),
            AgentPanelSort::Spaces
        );
    }

    #[test]
    fn agent_panel_sort_rejects_unknown_value() {
        let error = parse_agent_panel_sort(
            "[ui]\nagent_panel_sort = \"recent\"",
            Path::new("config.toml"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("ui.agent_panel_sort"));
    }

    const WORKSPACES: &str = r#"{"result":{"workspaces":[
        {"workspace_id":"w1","label":"dotfiles","tab_count":1},
        {"workspace_id":"w2","label":"dealon","tab_count":2}
    ]}}"#;
    const TABS: &str = r#"{"result":{"tabs":[
        {"tab_id":"w1:t1","workspace_id":"w1","label":"1","number":1},
        {"tab_id":"w2:t1","workspace_id":"w2","label":"1","number":1},
        {"tab_id":"w2:t2","workspace_id":"w2","label":"skills","number":2}
    ]}}"#;

    fn agent_with(workspace_id: &str, tab_id: &str) -> Agent {
        parse_agent_list(&format!(
            r#"{{"result":{{"agents":[{{"pane_id":"p1","terminal_id":"t1",
             "workspace_id":"{workspace_id}","tab_id":"{tab_id}"}}]}}}}"#
        ))
        .unwrap()
        .remove(0)
    }

    #[test]
    fn location_hides_tab_for_single_auto_named_tab() {
        let index = WorkspaceIndex::parse(WORKSPACES, TABS).unwrap();
        assert_eq!(index.location_for(&agent_with("w1", "w1:t1")), "dotfiles");
    }

    #[test]
    fn location_shows_auto_named_tab_when_workspace_has_multiple_tabs() {
        let index = WorkspaceIndex::parse(WORKSPACES, TABS).unwrap();
        assert_eq!(index.location_for(&agent_with("w2", "w2:t1")), "dealon · 1");
    }

    #[test]
    fn location_shows_custom_named_tab_even_when_only_tab() {
        let index = WorkspaceIndex::parse(WORKSPACES, TABS).unwrap();
        assert_eq!(
            index.location_for(&agent_with("w2", "w2:t2")),
            "dealon · skills"
        );
    }

    #[test]
    fn location_falls_back_to_dash_for_unknown_workspace() {
        let index = WorkspaceIndex::parse(WORKSPACES, TABS).unwrap();
        assert_eq!(index.location_for(&agent_with("wZ", "wZ:t1")), "-");
    }
}
