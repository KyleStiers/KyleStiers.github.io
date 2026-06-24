use std::{cell::RefCell, io, rc::Rc};

use ratzilla::backend::webgl2::{FontAtlasConfig, WebGl2Backend, WebGl2BackendOptions};
use ratzilla::event::KeyCode;
use ratzilla::ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use ratzilla::WebRenderer;
use serde::Deserialize;

#[cfg(target_arch = "wasm32")]
use js_sys::{Function, Reflect};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, JsValue};

// Provide a no-op critical-section implementation for wasm32-unknown-unknown,
// which is always single-threaded. Without this the linker emits bare `env`
// imports (_critical_section_1_0_acquire/_release) that browsers cannot resolve.
#[cfg(target_arch = "wasm32")]
struct WasmCriticalSection;
#[cfg(target_arch = "wasm32")]
critical_section::set_impl!(WasmCriticalSection);
#[cfg(target_arch = "wasm32")]
unsafe impl critical_section::Impl for WasmCriticalSection {
    unsafe fn acquire() {}
    unsafe fn release(_: ()) {}
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct ResumeSection {
    title: String,
    body: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct ResumeHeader {
    title: String,
    subtitle: String,
    download_label: String,
    download_url: String,
    download_filename: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct ResumeFooter {
    hint: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct Contact {
    scholar: String,
    github: String,
    linkedin: String,
    location: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct Profile {
    summary: Vec<String>,
    highlights: Vec<String>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct ExperienceEntry {
    organization: String,
    role: String,
    dates: String,
    manager: String,
    bullets: Vec<String>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct ResearchEntry {
    organization: String,
    role: String,
    mentor: String,
    publications: String,
    bullets: Vec<String>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct Skills {
    software: Vec<String>,
    molecular_biology: Vec<String>,
    structural_biology: Vec<String>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct EducationEntry {
    degree: String,
    institution: String,
    dates: String,
    notes: Vec<String>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct PositionEntry {
    title: String,
    organization: String,
    mentor: String,
    manager: String,
}

impl PositionEntry {
    fn guide_label_and_name(&self) -> Option<(&str, &str)> {
        if !self.manager.trim().is_empty() {
            Some(("Manager", self.manager.trim()))
        } else if !self.mentor.trim().is_empty() {
            Some(("Mentor", self.mentor.trim()))
        } else {
            None
        }
    }
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct ResumeData {
    header: ResumeHeader,
    contact: Contact,
    profile: Profile,
    experience: Vec<ExperienceEntry>,
    research: Vec<ResearchEntry>,
    skills: Skills,
    education: Vec<EducationEntry>,
    awards: Vec<String>,
    positions: Vec<PositionEntry>,
    footer: ResumeFooter,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Tui,
    Cli,
}

/// A node in the virtual resume filesystem explored from the CLI.
#[derive(Clone)]
enum CliNodeKind {
    Dir,
    File(String),
}

#[derive(Clone)]
struct CliNode {
    name: String,
    kind: CliNodeKind,
    children: Vec<CliNode>,
}

impl CliNode {
    fn dir(name: &str, children: Vec<CliNode>) -> Self {
        Self {
            name: name.to_string(),
            kind: CliNodeKind::Dir,
            children,
        }
    }

    fn file(name: &str, content: String) -> Self {
        Self {
            name: name.to_string(),
            kind: CliNodeKind::File(content),
            children: Vec::new(),
        }
    }

    fn is_dir(&self) -> bool {
        matches!(self.kind, CliNodeKind::Dir)
    }

    fn child(&self, name: &str) -> Option<&CliNode> {
        self.children.iter().find(|c| c.name == name)
    }
}

/// Visual styling category for a rendered CLI scrollback line.
#[derive(Clone, Copy)]
enum CliLineKind {
    Banner,
    Prompt,
    Output,
    Accent,
    Error,
    Muted,
}

struct CliState {
    input: String,
    cwd: Vec<String>,
    lines: Vec<(CliLineKind, String)>,
    history: Vec<String>,
    history_pos: Option<usize>,
}

impl CliState {
    fn new() -> Self {
        let mut cli = Self {
            input: String::new(),
            cwd: Vec::new(),
            lines: Vec::new(),
            history: Vec::new(),
            history_pos: None,
        };
        cli.seed_welcome();
        cli
    }

    fn push(&mut self, kind: CliLineKind, text: impl AsRef<str>) {
        for line in text.as_ref().split('\n') {
            self.lines.push((kind, line.to_string()));
        }
    }

    fn seed_welcome(&mut self) {
        self.push(CliLineKind::Banner, "kyle — interactive resume shell");
        self.push(
            CliLineKind::Muted,
            "Type 'help' for commands, 'ls' to look around, 'tui' to return to the visual UI.",
        );
        self.push(CliLineKind::Muted, "");
    }
}

/// Side effect a CLI command may request from the host app.
enum CliEffect {
    None,
    SwitchToTui,
}

struct AppState {
    mode: AppMode,
    selected_section: usize,
    resume: ResumeData,
    sections: Vec<ResumeSection>,
    cli: CliState,
    cli_root: CliNode,
    last_cols: u16,
    last_nav_width: u16,
    ui_scale: f64,
}

impl AppState {
    fn next(&mut self) {
        self.selected_section = (self.selected_section + 1) % self.sections.len();
    }

    fn previous(&mut self) {
        if self.selected_section == 0 {
            self.selected_section = self.sections.len() - 1;
        } else {
            self.selected_section -= 1;
        }
    }

    fn first(&mut self) {
        self.selected_section = 0;
    }

    fn last(&mut self) {
        self.selected_section = self.sections.len() - 1;
    }
}

fn bullets(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| format!("- {line}"))
        .collect::<Vec<String>>()
        .join("\n")
}

#[cfg(target_arch = "wasm32")]
fn open_external_link(url: &str) {
    if let Some(window) = ratzilla::web_sys::window() {
        let _ = window.open_with_url_and_target(url, "_blank");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open_external_link(_url: &str) {}

const LINKS_LINE: &str = "Links: LinkedIn | GitHub | Scholar";
const NAV_WIDTH_COLS: u16 = 30;
const HEADER_HEIGHT_ROWS: u16 = 5;
const HEADER_BUTTON_ROW: u16 = 1;
const HEADER_LINKS_ROW: u16 = 2;
const DEFAULT_FONT_SIZE: f32 = 22.0;

const BG_BASE: Color = Color::Rgb(24, 34, 49);
const SURFACE: Color = Color::Rgb(43, 54, 71);
const SURFACE_ALT: Color = Color::Rgb(54, 68, 89);
const TEXT_PRIMARY: Color = Color::Rgb(222, 229, 239);
const TEXT_MUTED: Color = Color::Rgb(167, 178, 196);
const ACCENT_BLUE: Color = Color::Rgb(136, 192, 208);
const LINK_YELLOW: Color = Color::Rgb(214, 195, 124);
const LINK_ORANGE: Color = Color::Rgb(210, 156, 110);
const LINK_RED: Color = Color::Rgb(195, 121, 125);

const BG_TERMINAL: Color = Color::Rgb(15, 21, 32);
const CLI_GREEN: Color = Color::Rgb(126, 200, 140);

const PANEL_BORDER_STYLE: Style = Style::new().fg(ACCENT_BLUE);
const PANEL_TITLE_STYLE: Style = Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD);

fn has_download_url(header: &ResumeHeader) -> bool {
    !header.download_url.trim().is_empty()
}

#[cfg(target_arch = "wasm32")]
fn is_download_enabled(_header: &ResumeHeader) -> bool {
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn is_download_enabled(header: &ResumeHeader) -> bool {
    has_download_url(header)
}

fn download_button_style(is_enabled: bool) -> Style {
    if is_enabled {
        Style::new()
            .fg(ACCENT_BLUE)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::new().fg(TEXT_MUTED).add_modifier(Modifier::BOLD)
    }
}

fn download_button_text(header: &ResumeHeader) -> String {
    let label = if header.download_label.trim().is_empty() {
        "Download CV"
    } else {
        header.download_label.trim()
    };

    format!("[{label}]")
}

fn mode_toggle_text(mode: AppMode) -> String {
    match mode {
        AppMode::Tui => "[ >_ Terminal ]".to_string(),
        AppMode::Cli => "[ TUI ]".to_string(),
    }
}

#[cfg(target_arch = "wasm32")]
fn set_resume_scale(scale: f64) {
    let clamped = scale.clamp(0.85, 1.5);

    let Some(window) = ratzilla::web_sys::window() else {
        return;
    };
    let window_js: JsValue = window.into();
    let Ok(setter) = Reflect::get(&window_js, &JsValue::from_str("setResumeScale")) else {
        return;
    };
    let Some(func) = setter.dyn_ref::<Function>() else {
        return;
    };

    let _ = func.call1(&JsValue::NULL, &JsValue::from_f64(clamped));
}

#[cfg(not(target_arch = "wasm32"))]
fn set_resume_scale(_scale: f64) {}

#[cfg(target_arch = "wasm32")]
fn markdown_bullets(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| format!("- {line}"))
        .collect::<Vec<String>>()
        .join("\n")
}

#[cfg(target_arch = "wasm32")]
fn pdf_filename(resume: &ResumeData) -> String {
    let from_toml = resume.header.download_filename.trim();
    if !from_toml.is_empty() {
        if from_toml.to_ascii_lowercase().ends_with(".pdf") {
            return from_toml.to_string();
        }
        return format!("{from_toml}.pdf");
    }

    let normalized = resume
        .header
        .title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let compact = normalized
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("_");

    if compact.is_empty() {
        "resume.pdf".to_string()
    } else {
        format!("{compact}.pdf")
    }
}

#[cfg(target_arch = "wasm32")]
fn build_resume_markdown(resume: &ResumeData) -> String {
    let mut out = String::new();

    out.push_str(&format!("# {}\n\n", resume.header.title));
    out.push_str(&format!("{}\n\n", resume.header.subtitle));
    out.push_str("Links: LinkedIn | GitHub | Scholar\n");
    out.push_str(&format!("LinkedIn: {}\n", resume.contact.linkedin));
    out.push_str(&format!("GitHub: {}\n", resume.contact.github));
    out.push_str(&format!("Scholar: {}\n", resume.contact.scholar));
    out.push_str(&format!("Location: {}\n\n", resume.contact.location));

    out.push_str("## Profile\n");
    for line in &resume.profile.summary {
        out.push_str(line);
        out.push_str("\n\n");
    }
    out.push_str("### Highlights\n");
    out.push_str(&markdown_bullets(&resume.profile.highlights));
    out.push_str("\n\n");

    out.push_str("## Data Science Experience\n");
    for entry in &resume.experience {
        out.push_str(&format!(
            "### {} | {}\n{}\nManager: {}\n{}\n\n",
            entry.role,
            entry.organization,
            entry.dates,
            entry.manager,
            markdown_bullets(&entry.bullets)
        ));
    }

    out.push_str("## Research Experience\n");
    for entry in &resume.research {
        out.push_str(&format!(
            "### {} | {}\nMentor: {}\nPublications: {}\n{}\n\n",
            entry.role,
            entry.organization,
            entry.mentor,
            entry.publications,
            markdown_bullets(&entry.bullets)
        ));
    }

    out.push_str("## Technical Skills\n");
    out.push_str("### Software Development and Computing\n");
    out.push_str(&markdown_bullets(&resume.skills.software));
    out.push_str("\n\n### Molecular Biology and Protein Biochemistry\n");
    out.push_str(&markdown_bullets(&resume.skills.molecular_biology));
    out.push_str("\n\n### Structural Biology and Biophysics\n");
    out.push_str(&markdown_bullets(&resume.skills.structural_biology));
    out.push_str("\n\n");

    out.push_str("## Education\n");
    for entry in &resume.education {
        out.push_str(&format!(
            "### {}\n{}\n{}\n{}\n\n",
            entry.degree,
            entry.institution,
            entry.dates,
            markdown_bullets(&entry.notes)
        ));
    }

    out.push_str("## Awards\n");
    out.push_str(&markdown_bullets(&resume.awards));
    out.push_str("\n\n");

    out.push_str("## Positions\n");
    for entry in &resume.positions {
        if let Some((label, name)) = entry.guide_label_and_name() {
            out.push_str(&format!(
                "- {} | {} ({}: {})\n",
                entry.title, entry.organization, label, name
            ));
        } else {
            out.push_str(&format!("- {} | {}\n", entry.title, entry.organization));
        }
    }
    out.push('\n');

    out
}

#[cfg(target_arch = "wasm32")]
fn download_generated_resume(resume: &ResumeData) {
    let content = build_resume_markdown(resume);
    let file_name = pdf_filename(resume);

    let Some(window) = ratzilla::web_sys::window() else {
        return;
    };

    let window_js: JsValue = window.into();
    let Ok(generator) = Reflect::get(&window_js, &JsValue::from_str("generateResumePdf")) else {
        return;
    };

    let Some(func) = generator.dyn_ref::<Function>() else {
        return;
    };

    let _ = func.call2(
        &JsValue::NULL,
        &JsValue::from_str(&content),
        &JsValue::from_str(&file_name),
    );
}

#[cfg(target_arch = "wasm32")]
fn trigger_download_action(resume: &ResumeData) {
    if has_download_url(&resume.header) {
        open_external_link(resume.header.download_url.trim());
    } else {
        download_generated_resume(resume);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn trigger_download_action(resume: &ResumeData) {
    if has_download_url(&resume.header) {
        open_external_link(resume.header.download_url.trim());
    }
}

fn render_sections(resume: &ResumeData) -> Vec<ResumeSection> {
    let mut sections = Vec::new();

    sections.push(ResumeSection {
        title: "Profile".to_string(),
        body: format!(
            "{}\n\nHighlights\n{}",
            resume.profile.summary.join("\n\n"),
            bullets(&resume.profile.highlights)
        ),
    });

    let experience_body = resume
        .experience
        .iter()
        .map(|entry| {
            format!(
                "{} | {}\nManager: {}\n{}\n{}",
                entry.role,
                entry.organization,
                entry.manager,
                entry.dates,
                bullets(&entry.bullets)
            )
        })
        .collect::<Vec<String>>()
        .join("\n\n");
    sections.push(ResumeSection {
        title: "Data Science Experience".to_string(),
        body: experience_body,
    });

    let research_body = resume
        .research
        .iter()
        .map(|entry| {
            format!(
                "{} | {}\nMentor: {}\nPublications: {}\n{}",
                entry.role,
                entry.organization,
                entry.mentor,
                entry.publications,
                bullets(&entry.bullets)
            )
        })
        .collect::<Vec<String>>()
        .join("\n\n");
    sections.push(ResumeSection {
        title: "Research Experience".to_string(),
        body: research_body,
    });

    sections.push(ResumeSection {
        title: "Technical Skills".to_string(),
        body: format!(
            "Software Development and Computing\n{}\n\nMolecular Biology and Protein Biochemistry\n{}\n\nStructural Biology and Biophysics\n{}",
            bullets(&resume.skills.software),
            bullets(&resume.skills.molecular_biology),
            bullets(&resume.skills.structural_biology)
        ),
    });

    let education_body = resume
        .education
        .iter()
        .map(|entry| {
            format!(
                "{}\n{}\n{}\n{}",
                entry.degree,
                entry.institution,
                entry.dates,
                bullets(&entry.notes)
            )
        })
        .collect::<Vec<String>>()
        .join("\n\n");
    sections.push(ResumeSection {
        title: "Education".to_string(),
        body: education_body,
    });

    sections.push(ResumeSection {
        title: "Awards".to_string(),
        body: bullets(&resume.awards),
    });

    let positions_body = resume
        .positions
        .iter()
        .map(|entry| match entry.guide_label_and_name() {
            Some((label, name)) => {
                format!("{}\n{}\n{}: {}", entry.title, entry.organization, label, name)
            }
            None => format!("{}\n{}", entry.title, entry.organization),
        })
        .collect::<Vec<String>>()
        .join("\n\n");
    sections.push(ResumeSection {
        title: "Positions".to_string(),
        body: positions_body,
    });

    sections.push(ResumeSection {
        title: "Contact".to_string(),
        body: format!(
            "GitHub: {}\nLinkedIn: {}\nGoogle Scholar: {}\nLocation: {}",
            resume.contact.github,
            resume.contact.linkedin,
            resume.contact.scholar,
            resume.contact.location
        ),
    });

    sections
}

fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        out.push_str("item");
    }
    out
}

fn unique_name(used: &mut Vec<String>, base: String) -> String {
    if !used.contains(&base) {
        used.push(base.clone());
        return base;
    }
    let mut i = 2;
    loop {
        let candidate = format!("{base}-{i}");
        if !used.contains(&candidate) {
            used.push(candidate.clone());
            return candidate;
        }
        i += 1;
    }
}

/// Builds the virtual filesystem the CLI explorer walks, sourced entirely
/// from the already-parsed resume data.
fn build_cli_tree(resume: &ResumeData) -> CliNode {
    let mut root = Vec::new();

    root.push(CliNode::dir(
        "profile",
        vec![
            CliNode::file("summary", resume.profile.summary.join("\n\n")),
            CliNode::file("highlights", bullets(&resume.profile.highlights)),
        ],
    ));

    let mut exp_used = Vec::new();
    let experience: Vec<CliNode> = resume
        .experience
        .iter()
        .map(|entry| {
            let name = unique_name(&mut exp_used, slugify(&entry.organization));
            let content = format!(
                "{} | {}\n{}\nManager: {}\n\n{}",
                entry.role,
                entry.organization,
                entry.dates,
                entry.manager,
                bullets(&entry.bullets)
            );
            CliNode::file(&name, content)
        })
        .collect();
    root.push(CliNode::dir("experience", experience));

    let mut research_used = Vec::new();
    let research: Vec<CliNode> = resume
        .research
        .iter()
        .map(|entry| {
            let name = unique_name(&mut research_used, slugify(&entry.role));
            let content = format!(
                "{} | {}\nMentor: {}\nPublications: {}\n\n{}",
                entry.role,
                entry.organization,
                entry.mentor,
                entry.publications,
                bullets(&entry.bullets)
            );
            CliNode::file(&name, content)
        })
        .collect();
    root.push(CliNode::dir("research", research));

    root.push(CliNode::dir(
        "skills",
        vec![
            CliNode::file("software", bullets(&resume.skills.software)),
            CliNode::file("molecular-biology", bullets(&resume.skills.molecular_biology)),
            CliNode::file("structural-biology", bullets(&resume.skills.structural_biology)),
        ],
    ));

    let mut edu_used = Vec::new();
    let education: Vec<CliNode> = resume
        .education
        .iter()
        .map(|entry| {
            let name = unique_name(&mut edu_used, slugify(&entry.degree));
            let content = format!(
                "{}\n{}\n{}\n\n{}",
                entry.degree,
                entry.institution,
                entry.dates,
                bullets(&entry.notes)
            );
            CliNode::file(&name, content)
        })
        .collect();
    root.push(CliNode::dir("education", education));

    root.push(CliNode::file("awards", bullets(&resume.awards)));

    let positions = resume
        .positions
        .iter()
        .map(|entry| match entry.guide_label_and_name() {
            Some((label, name)) => {
                format!("{}\n  {}\n  {}: {}", entry.title, entry.organization, label, name)
            }
            None => format!("{}\n  {}", entry.title, entry.organization),
        })
        .collect::<Vec<String>>()
        .join("\n\n");
    root.push(CliNode::file("positions", positions));

    root.push(CliNode::file(
        "contact",
        format!(
            "GitHub:   {}\nLinkedIn: {}\nScholar:  {}\nLocation: {}",
            resume.contact.github,
            resume.contact.linkedin,
            resume.contact.scholar,
            resume.contact.location
        ),
    ));

    CliNode::dir("", root)
}

fn cli_path_string(cwd: &[String]) -> String {
    if cwd.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", cwd.join("/"))
    }
}

fn cli_prompt_prefix(cwd: &[String]) -> String {
    format!("kyle@cv:{}$ ", cli_path_string(cwd))
}

fn cli_node_at<'a>(root: &'a CliNode, path: &[String]) -> Option<&'a CliNode> {
    let mut current = root;
    for segment in path {
        current = current.child(segment)?;
    }
    Some(current)
}

/// Resolves a user-supplied path (absolute or relative, with `.`/`..`)
/// against the tree, returning the resolved segment list if every hop exists.
fn cli_resolve(root: &CliNode, cwd: &[String], arg: &str) -> Result<Vec<String>, String> {
    let mut path: Vec<String> = if arg.starts_with('/') {
        Vec::new()
    } else {
        cwd.to_vec()
    };

    for segment in arg.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                path.pop();
            }
            name => {
                let here = cli_node_at(root, &path)
                    .ok_or_else(|| "no such file or directory".to_string())?;
                match here.child(name) {
                    Some(_) => path.push(name.to_string()),
                    None => return Err(format!("no such file or directory: {name}")),
                }
            }
        }
    }

    Ok(path)
}

fn cli_tree_lines(node: &CliNode, prefix: &str, out: &mut Vec<(CliLineKind, String)>) {
    let count = node.children.len();
    for (index, child) in node.children.iter().enumerate() {
        let last = index + 1 == count;
        let branch = if last { "└── " } else { "├── " };
        let (kind, suffix) = if child.is_dir() {
            (CliLineKind::Accent, "/")
        } else {
            (CliLineKind::Output, "")
        };
        out.push((kind, format!("{prefix}{branch}{}{suffix}", child.name)));
        if child.is_dir() {
            let extension = if last { "    " } else { "│   " };
            cli_tree_lines(child, &format!("{prefix}{extension}"), out);
        }
    }
}

fn cli_help_lines() -> Vec<(CliLineKind, String)> {
    let entries = [
        ("help", "show this help"),
        ("ls [path]", "list entries in a directory"),
        ("cd [path]", "change directory ('cd ..' / 'cd /')"),
        ("cat <path>", "print a resume section"),
        ("pwd", "print the working directory"),
        ("tree [path]", "show the directory tree"),
        ("whoami", "who is this person"),
        ("open <name>", "open a link: linkedin | github | scholar"),
        ("download", "download the CV as a PDF"),
        ("clear", "clear the screen"),
        ("tui | exit", "return to the visual UI"),
    ];

    let mut lines = vec![(CliLineKind::Accent, "Available commands:".to_string())];
    for (cmd, desc) in entries {
        lines.push((CliLineKind::Output, format!("  {cmd:<14}{desc}")));
    }
    lines.push((CliLineKind::Muted, String::new()));
    lines.push((
        CliLineKind::Muted,
        "Tip: try `ls`, then `cat profile/summary`.".to_string(),
    ));
    lines
}

/// Executes a single CLI command line, mutating scrollback/cwd and returning
/// any host-level side effect (e.g. switching back to the visual UI).
fn cli_execute(cli: &mut CliState, root: &CliNode, resume: &ResumeData) -> CliEffect {
    let raw = std::mem::take(&mut cli.input);
    let prompt = cli_prompt_prefix(&cli.cwd);
    cli.lines.push((CliLineKind::Prompt, format!("{prompt}{raw}")));

    let line = raw.trim();
    if line.is_empty() {
        return CliEffect::None;
    }

    cli.history.push(line.to_string());
    cli.history_pos = None;

    let mut parts = line.split_whitespace();
    let command = parts.next().unwrap_or("");
    let args: Vec<&str> = parts.collect();

    match command {
        "help" | "?" => {
            for entry in cli_help_lines() {
                cli.lines.push(entry);
            }
        }
        "pwd" => cli.push(CliLineKind::Output, cli_path_string(&cli.cwd)),
        "clear" => cli.lines.clear(),
        "whoami" => {
            cli.push(CliLineKind::Accent, resume.header.title.clone());
            cli.push(CliLineKind::Output, resume.header.subtitle.clone());
            cli.push(
                CliLineKind::Muted,
                format!("Location: {}", resume.contact.location),
            );
        }
        "ls" => {
            let target = args.first().copied().unwrap_or("");
            let path = match cli_resolve(root, &cli.cwd, target) {
                Ok(path) => path,
                Err(err) => {
                    cli.push(CliLineKind::Error, format!("ls: {err}"));
                    return CliEffect::None;
                }
            };
            let node = cli_node_at(root, &path).unwrap();
            match &node.kind {
                CliNodeKind::File(_) => cli.push(CliLineKind::Output, node.name.clone()),
                CliNodeKind::Dir => {
                    if node.children.is_empty() {
                        cli.push(CliLineKind::Muted, "(empty)");
                    }
                    for child in &node.children {
                        if child.is_dir() {
                            cli.lines
                                .push((CliLineKind::Accent, format!("{}/", child.name)));
                        } else {
                            cli.lines.push((CliLineKind::Output, child.name.clone()));
                        }
                    }
                }
            }
        }
        "cd" => {
            let target = args.first().copied().unwrap_or("/");
            match cli_resolve(root, &cli.cwd, target) {
                Ok(path) => {
                    let node = cli_node_at(root, &path).unwrap();
                    if node.is_dir() {
                        cli.cwd = path;
                    } else {
                        cli.push(CliLineKind::Error, format!("cd: not a directory: {target}"));
                    }
                }
                Err(err) => cli.push(CliLineKind::Error, format!("cd: {err}")),
            }
        }
        "cat" => {
            if args.is_empty() {
                cli.push(CliLineKind::Error, "cat: missing operand (try 'ls')");
                return CliEffect::None;
            }
            for target in &args {
                match cli_resolve(root, &cli.cwd, target) {
                    Ok(path) => {
                        let node = cli_node_at(root, &path).unwrap();
                        match &node.kind {
                            CliNodeKind::File(content) => {
                                cli.push(CliLineKind::Output, content.clone())
                            }
                            CliNodeKind::Dir => cli.push(
                                CliLineKind::Error,
                                format!("cat: {target}: is a directory (try 'ls {target}')"),
                            ),
                        }
                    }
                    Err(err) => cli.push(CliLineKind::Error, format!("cat: {err}")),
                }
            }
        }
        "tree" => {
            let target = args.first().copied().unwrap_or("");
            match cli_resolve(root, &cli.cwd, target) {
                Ok(path) => {
                    let node = cli_node_at(root, &path).unwrap();
                    let label = cli_path_string(&path);
                    cli.push(CliLineKind::Accent, label);
                    let mut out = Vec::new();
                    cli_tree_lines(node, "", &mut out);
                    for entry in out {
                        cli.lines.push(entry);
                    }
                }
                Err(err) => cli.push(CliLineKind::Error, format!("tree: {err}")),
            }
        }
        "open" => match args.first().copied() {
            Some("linkedin") => {
                cli.push(CliLineKind::Muted, "opening LinkedIn...");
                open_external_link(resume.contact.linkedin.trim());
            }
            Some("github") => {
                cli.push(CliLineKind::Muted, "opening GitHub...");
                open_external_link(resume.contact.github.trim());
            }
            Some("scholar") => {
                cli.push(CliLineKind::Muted, "opening Google Scholar...");
                open_external_link(resume.contact.scholar.trim());
            }
            Some(other) => {
                cli.push(CliLineKind::Error, format!("open: unknown target: {other}"));
            }
            None => cli.push(
                CliLineKind::Error,
                "open: missing target (linkedin | github | scholar)",
            ),
        },
        "download" => {
            cli.push(CliLineKind::Muted, "preparing CV download...");
            trigger_download_action(resume);
        }
        "tui" | "exit" | "gui" | "visual" | "q" => return CliEffect::SwitchToTui,
        other => cli.push(
            CliLineKind::Error,
            format!("command not found: {other} (try 'help')"),
        ),
    }

    CliEffect::None
}

/// Longest string that is a prefix of every candidate name.
fn cli_common_prefix(items: &[(String, bool)]) -> String {
    let Some((first, _)) = items.first() else {
        return String::new();
    };
    let mut prefix = first.clone();
    for (name, _) in &items[1..] {
        while !name.starts_with(&prefix) {
            prefix.pop();
            if prefix.is_empty() {
                return String::new();
            }
        }
    }
    prefix
}

/// Applies a completion result to the input buffer: fills a unique match,
/// extends to the common prefix, or echoes the candidate list.
fn cli_apply_completion(
    cli: &mut CliState,
    full_prefix: &str,
    frag: &str,
    candidates: &[(String, bool)],
) {
    match candidates.len() {
        0 => {}
        1 => {
            let (name, is_dir) = &candidates[0];
            let suffix = if *is_dir { "/" } else { " " };
            cli.input = format!("{full_prefix}{name}{suffix}");
        }
        _ => {
            let common = cli_common_prefix(candidates);
            if common.len() > frag.len() {
                cli.input = format!("{full_prefix}{common}");
            } else {
                let current = cli.input.clone();
                let prompt = cli_prompt_prefix(&cli.cwd);
                cli.lines
                    .push((CliLineKind::Prompt, format!("{prompt}{current}")));
                let listing = candidates
                    .iter()
                    .map(|(name, is_dir)| if *is_dir { format!("{name}/") } else { name.clone() })
                    .collect::<Vec<_>>()
                    .join("  ");
                cli.push(CliLineKind::Muted, listing);
            }
        }
    }
}

/// Tab-completes the current input: command names for the first token,
/// filesystem paths (with `.`/`..`) for subsequent tokens.
fn cli_complete(cli: &mut CliState, root: &CliNode) {
    let input = cli.input.clone();

    let (prefix, last) = match input.rfind(char::is_whitespace) {
        Some(idx) => (input[..=idx].to_string(), input[idx + 1..].to_string()),
        None => (String::new(), input.clone()),
    };

    // First token: complete against the command vocabulary.
    if prefix.is_empty() {
        const COMMANDS: [&str; 12] = [
            "help", "ls", "cd", "cat", "pwd", "tree", "whoami", "open", "download", "clear",
            "tui", "exit",
        ];
        let candidates: Vec<(String, bool)> = COMMANDS
            .iter()
            .filter(|c| c.starts_with(&last))
            .map(|c| (c.to_string(), false))
            .collect();
        cli_apply_completion(cli, &prefix, &last, &candidates);
        return;
    }

    // Subsequent tokens: complete a filesystem path.
    let (dir_part, frag) = match last.rfind('/') {
        Some(idx) => (last[..=idx].to_string(), last[idx + 1..].to_string()),
        None => (String::new(), last.clone()),
    };

    let base = if dir_part.is_empty() { "." } else { &dir_part };
    let Ok(path) = cli_resolve(root, &cli.cwd, base) else {
        return;
    };
    let Some(node) = cli_node_at(root, &path) else {
        return;
    };
    if !node.is_dir() {
        return;
    }

    let candidates: Vec<(String, bool)> = node
        .children
        .iter()
        .filter(|child| child.name.starts_with(&frag))
        .map(|child| (child.name.clone(), child.is_dir()))
        .collect();

    let full_prefix = format!("{prefix}{dir_part}");
    cli_apply_completion(cli, &full_prefix, &frag, &candidates);
}

fn cli_line_style(kind: CliLineKind) -> Style {
    match kind {
        CliLineKind::Banner => Style::default()
            .fg(ACCENT_BLUE)
            .add_modifier(Modifier::BOLD),
        CliLineKind::Prompt => Style::default().fg(TEXT_MUTED),
        CliLineKind::Output => Style::default().fg(TEXT_PRIMARY),
        CliLineKind::Accent => Style::default()
            .fg(CLI_GREEN)
            .add_modifier(Modifier::BOLD),
        CliLineKind::Error => Style::default().fg(LINK_RED),
        CliLineKind::Muted => Style::default().fg(TEXT_MUTED),
    }
}

/// Number of rows a logical line occupies once soft-wrapped to `width`.
fn wrapped_rows(char_len: usize, width: u16) -> usize {
    let width = width.max(1) as usize;
    if char_len == 0 {
        1
    } else {
        char_len.div_ceil(width)
    }
}

/// Renders the emulated terminal: a bordered shell panel with bottom-pinned
/// scrollback and an active prompt line with a block cursor.
fn render_cli(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let block = Block::default()
        .title(" kyle@cv : terminal ")
        .title_style(
            Style::default()
                .fg(CLI_GREEN)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLI_GREEN))
        .style(Style::default().bg(BG_TERMINAL));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    for (kind, text) in &state.cli.lines {
        let style = cli_line_style(*kind);
        if text.is_empty() {
            lines.push(Line::from(""));
        } else {
            for segment in text.split('\n') {
                lines.push(Line::styled(segment.to_string(), style));
            }
        }
    }

    // Active prompt line: green host, blue path, primary input, block cursor.
    let path = cli_path_string(&state.cli.cwd);
    let prompt_spans = vec![
        Span::styled("kyle@cv", Style::default().fg(CLI_GREEN)),
        Span::styled(":", Style::default().fg(TEXT_MUTED)),
        Span::styled(path, Style::default().fg(ACCENT_BLUE)),
        Span::styled("$ ", Style::default().fg(TEXT_MUTED)),
        Span::styled(state.cli.input.clone(), Style::default().fg(TEXT_PRIMARY)),
        Span::styled(
            "█",
            Style::default().fg(ACCENT_BLUE).add_modifier(Modifier::SLOW_BLINK),
        ),
    ];
    lines.push(Line::from(prompt_spans));

    // Pin the view to the bottom by scrolling past overflowing rows.
    let total_rows: usize = lines
        .iter()
        .map(|line| wrapped_rows(line.width(), inner.width))
        .sum();
    let visible = inner.height as usize;
    let scroll_y = total_rows.saturating_sub(visible) as u16;

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(BG_TERMINAL))
        .scroll((scroll_y, 0));
    frame.render_widget(paragraph, inner);
}

fn build_app_state() -> AppState {
    let source = include_str!("../resume.toml");

    match toml::from_str::<ResumeData>(source) {
        Ok(resume) => AppState {
            mode: AppMode::Tui,
            selected_section: 0,
            sections: render_sections(&resume),
            cli: CliState::new(),
            cli_root: build_cli_tree(&resume),
            resume,
            last_cols: 120,
            last_nav_width: NAV_WIDTH_COLS,
            ui_scale: 1.0,
        },
        Err(err) => {
            let resume = ResumeData {
                header: ResumeHeader {
                    title: "Resume Site Error".to_string(),
                    subtitle: "Failed to parse resume.toml".to_string(),
                    download_label: String::new(),
                    download_url: String::new(),
                    download_filename: String::new(),
                },
                contact: Contact {
                    scholar: String::new(),
                    github: String::new(),
                    linkedin: String::new(),
                    location: String::new(),
                },
                profile: Profile {
                    summary: vec![],
                    highlights: vec![],
                },
                experience: vec![],
                research: vec![],
                skills: Skills {
                    software: vec![],
                    molecular_biology: vec![],
                    structural_biology: vec![],
                },
                education: vec![],
                awards: vec![],
                positions: vec![],
                footer: ResumeFooter {
                    hint: "Fix resume.toml and rebuild. Navigation keys are disabled in fallback mode."
                        .to_string(),
                },
            };

            let sections = vec![ResumeSection {
                title: "Configuration Error".to_string(),
                body: format!("The application could not parse resume.toml.\n\n{}", err),
            }];

            AppState {
                mode: AppMode::Tui,
                selected_section: 0,
                cli: CliState::new(),
                cli_root: build_cli_tree(&resume),
                resume,
                sections,
                last_cols: 120,
                last_nav_width: NAV_WIDTH_COLS,
                ui_scale: 1.0,
            }
        }
    }
}

fn main() -> io::Result<()> {
    // Use a dynamic font atlas for smoother text rendering than the static default atlas.
    let backend = WebGl2Backend::new_with_options(
        WebGl2BackendOptions::new().font_atlas_config(FontAtlasConfig::dynamic(
            &["Fira Code", "Menlo", "Consolas", "monospace"],
            DEFAULT_FONT_SIZE,
        )),
    )?;
    let mut terminal = Terminal::new(backend)?;
    let app_state = build_app_state();

    if app_state.sections.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "resume.toml did not render any visible sections",
        ));
    }

    let state = Rc::new(RefCell::new(app_state));

    // Initialize browser-side scale to match state.
    {
        let st = state.borrow();
        set_resume_scale(st.ui_scale);
    }

    terminal.on_key_event({
        let state = state.clone();
        move |key_event| {
            let mut state = state.borrow_mut();
            let st = &mut *state;

            // CLI mode captures typing; handle it first and bail out.
            if st.mode == AppMode::Cli {
                match key_event.code {
                    KeyCode::Esc => st.mode = AppMode::Tui,
                    KeyCode::Enter => {
                        let effect = cli_execute(&mut st.cli, &st.cli_root, &st.resume);
                        if let CliEffect::SwitchToTui = effect {
                            st.mode = AppMode::Tui;
                        }
                    }
                    KeyCode::Backspace => {
                        st.cli.input.pop();
                    }
                    KeyCode::Tab => {
                        cli_complete(&mut st.cli, &st.cli_root);
                    }
                    KeyCode::Up => {
                        let next = match st.cli.history_pos {
                            Some(pos) => pos.saturating_sub(1),
                            None => st.cli.history.len().saturating_sub(1),
                        };
                        if !st.cli.history.is_empty() {
                            st.cli.history_pos = Some(next);
                            st.cli.input = st.cli.history[next].clone();
                        }
                    }
                    KeyCode::Down => match st.cli.history_pos {
                        Some(pos) if pos + 1 < st.cli.history.len() => {
                            st.cli.history_pos = Some(pos + 1);
                            st.cli.input = st.cli.history[pos + 1].clone();
                        }
                        Some(_) => {
                            st.cli.history_pos = None;
                            st.cli.input.clear();
                        }
                        None => {}
                    },
                    KeyCode::Char(c) => {
                        if key_event.ctrl {
                            if c == 'c' || c == 'C' {
                                // Terminal-style interrupt: echo ^C and start
                                // a fresh prompt, discarding the current line.
                                let prompt = cli_prompt_prefix(&st.cli.cwd);
                                let current = st.cli.input.clone();
                                st.cli
                                    .lines
                                    .push((CliLineKind::Prompt, format!("{prompt}{current}^C")));
                                st.cli.input.clear();
                                st.cli.history_pos = None;
                            } else if c == 'l' || c == 'L' {
                                st.cli.lines.clear();
                            } else if c == 'u' || c == 'U' {
                                st.cli.input.clear();
                            }
                        } else if !key_event.alt {
                            st.cli.input.push(c);
                        }
                    }
                    _ => {}
                }
                return;
            }

            match key_event.code {
                KeyCode::Down | KeyCode::Char('j') => st.next(),
                KeyCode::Up | KeyCode::Char('k') => st.previous(),
                KeyCode::Home => st.first(),
                KeyCode::End => st.last(),
                KeyCode::Char('t') => st.mode = AppMode::Cli,
                KeyCode::Char('d') => {
                    trigger_download_action(&st.resume);
                }
                KeyCode::Char('x') => {
                    st.ui_scale = (st.ui_scale + 0.1).clamp(0.85, 1.5);
                    set_resume_scale(st.ui_scale);
                }
                KeyCode::Char('z') => {
                    st.ui_scale = (st.ui_scale - 0.1).clamp(0.85, 1.5);
                    set_resume_scale(st.ui_scale);
                }
                KeyCode::Char('c') => {
                    st.ui_scale = 1.0;
                    set_resume_scale(st.ui_scale);
                }
                _ => {}
            }
        }
    })?;

    // Click on header actions or a nav list item to select a section.
    terminal.on_mouse_event({
        let state = state.clone();
        move |mouse_event| {
            use ratzilla::event::MouseEventKind;
            match mouse_event.kind {
                MouseEventKind::SingleClick(_) => {}
                _ => return,
            }

            // Header top-left button: toggle between visual UI and terminal.
            let toggle_hit = {
                let st = state.borrow();
                if mouse_event.row == HEADER_BUTTON_ROW {
                    let text = mode_toggle_text(st.mode);
                    let len = text.chars().count() as u16;
                    mouse_event.col < len
                } else {
                    false
                }
            };
            if toggle_hit {
                let mut st = state.borrow_mut();
                st.mode = match st.mode {
                    AppMode::Tui => AppMode::Cli,
                    AppMode::Cli => AppMode::Tui,
                };
                return;
            }

            // Header top-right button, generated from TOML header values.
            let maybe_download = {
                let st = state.borrow();
                if mouse_event.row == HEADER_BUTTON_ROW {
                    let button_text = download_button_text(&st.resume.header);
                    let button_len = button_text.chars().count() as u16;
                    let start_col = st.last_cols.saturating_sub(button_len);
                    let end_col = start_col.saturating_add(button_len);
                    if mouse_event.col >= start_col && mouse_event.col < end_col {
                        Some(())
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if maybe_download.is_some() {
                let st = state.borrow();
                trigger_download_action(&st.resume);
                return;
            }

            // Header links row: Links: LinkedIn | GitHub | Scholar
            let maybe_link = {
                let st = state.borrow();
                let links_line = LINKS_LINE;
                let line_len = links_line.chars().count() as u16;
                let start_col = st.last_cols.saturating_sub(line_len) / 2;
                let end_col = start_col.saturating_add(line_len);
                let in_links_row = mouse_event.row == HEADER_LINKS_ROW;

                if in_links_row && mouse_event.col >= start_col && mouse_event.col < end_col {
                    let rel = mouse_event.col - start_col;
                    if (7..15).contains(&rel) {
                        Some(st.resume.contact.linkedin.clone())
                    } else if (18..24).contains(&rel) {
                        Some(st.resume.contact.github.clone())
                    } else if (27..34).contains(&rel) {
                        Some(st.resume.contact.scholar.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if let Some(url) = maybe_link {
                open_external_link(&url);
                return;
            }

            // Only react to clicks in the left navigation panel.
            let nav_width = {
                let st = state.borrow();
                if st.mode == AppMode::Cli {
                    return;
                }
                st.last_nav_width
            };
            if mouse_event.col >= nav_width {
                return;
            }
            let state_ref = state.borrow();
            let section_count = state_ref.sections.len();
            drop(state_ref);

            // Row 0..HEADER_HEIGHT_ROWS-1 are header area, and the nav box top
            // border takes one additional row before list items start.
            let nav_items_start_row = HEADER_HEIGHT_ROWS + 1;
            if mouse_event.row < nav_items_start_row {
                return;
            }
            let idx = (mouse_event.row - nav_items_start_row) as usize;
            if idx < section_count {
                state.borrow_mut().selected_section = idx;
            }
        }
    })?;

    terminal.draw_web(move |frame| {
        let mut state = state.borrow_mut();
        let area = frame.area();
        state.last_cols = area.width;

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(HEADER_HEIGHT_ROWS),
                Constraint::Min(8),
                Constraint::Length(1),
            ])
            .split(area);

        let header_block = Block::default()
            .style(Style::default().bg(BG_BASE))
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(SURFACE_ALT));
        let header_inner = header_block.inner(layout[0]);
        frame.render_widget(header_block, layout[0]);

        let header_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(header_inner);

        let title = Paragraph::new(state.resume.header.title.clone())
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(ACCENT_BLUE)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(title, header_rows[0]);

        let button_text = download_button_text(&state.resume.header);
        let button_width = button_text.chars().count() as u16;
        let toggle_text = mode_toggle_text(state.mode);
        let toggle_width = toggle_text.chars().count() as u16;
        let subtitle_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(toggle_width),
                Constraint::Min(12),
                Constraint::Length(button_width),
            ])
            .split(header_rows[1]);

        let toggle = Paragraph::new(toggle_text)
            .alignment(Alignment::Left)
            .style(
                Style::default()
                    .fg(CLI_GREEN)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(toggle, subtitle_row[0]);

        let subtitle = Paragraph::new(state.resume.header.subtitle.clone())
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_PRIMARY));
        frame.render_widget(subtitle, subtitle_row[1]);

        let button = Paragraph::new(button_text)
            .alignment(Alignment::Right)
            .style(download_button_style(is_download_enabled(&state.resume.header)));
        frame.render_widget(button, subtitle_row[2]);

        let links = Paragraph::new(Line::from(vec![
            Span::styled("Links: ", Style::default().fg(TEXT_MUTED)),
            Span::styled(
                "LinkedIn",
                Style::default()
                    .fg(LINK_YELLOW)
                    .add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            ),
            Span::styled(" | ", Style::default().fg(TEXT_MUTED)),
            Span::styled(
                "GitHub",
                Style::default()
                    .fg(LINK_ORANGE)
                    .add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            ),
            Span::styled(" | ", Style::default().fg(TEXT_MUTED)),
            Span::styled(
                "Scholar",
                Style::default()
                    .fg(LINK_RED)
                    .add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            ),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(links, header_rows[2]);

        if state.mode == AppMode::Cli {
            render_cli(frame, layout[1], &mut state);
        } else {
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(
                        NAV_WIDTH_COLS
                            .min(area.width.saturating_sub(12))
                            .max(1),
                    ),
                    Constraint::Min(10),
                ])
                .split(layout[1]);

            state.last_nav_width = body[0].width;

            let nav_items: Vec<ListItem> = state
                .sections
                .iter()
                .enumerate()
                .map(|(index, section)| {
                    let prefix = if index == state.selected_section { "> " } else { "  " };
                    let style = if index == state.selected_section {
                        Style::default()
                            .fg(ACCENT_BLUE)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(TEXT_PRIMARY)
                    };
                    ListItem::new(format!("{prefix}{}", section.title)).style(style)
                })
                .collect();

            let navigation = List::new(nav_items).block(
                Block::default()
                    .title(" Sections ")
                    .title_style(PANEL_TITLE_STYLE)
                    .style(Style::default().bg(SURFACE))
                    .borders(Borders::ALL)
                    .border_style(PANEL_BORDER_STYLE),
            );
            frame.render_widget(navigation, body[0]);

            let content = &state.sections[state.selected_section];
            let details = Paragraph::new(content.body.clone())
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(TEXT_PRIMARY).bg(SURFACE))
                .block(
                    Block::default()
                        .title(format!(" {} ", content.title.clone()))
                        .title_style(PANEL_TITLE_STYLE)
                        .style(Style::default().bg(SURFACE))
                        .borders(Borders::ALL)
                        .border_style(PANEL_BORDER_STYLE),
                );
            frame.render_widget(details, body[1]);
        }

        let footer_text = if state.mode == AppMode::Cli {
            "Terminal mode | type 'help' for commands | Esc returns to the visual UI".to_string()
        } else {
            format!(
                "{} | Press 't' for terminal | Zoom: z/x (reset c) [{:.0}%]",
                state.resume.footer.hint,
                state.ui_scale * 100.0
            )
        };
        let footer = Paragraph::new(footer_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_MUTED).bg(BG_BASE));
        frame.render_widget(footer, layout[2]);
    });

    Ok(())
}
