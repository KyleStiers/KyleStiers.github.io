use std::{cell::RefCell, io, rc::Rc};

use ratzilla::backend::webgl2::{FontAtlasConfig, WebGl2Backend, WebGl2BackendOptions};
use ratzilla::event::KeyCode;
use ratzilla::ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Terminal,
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

struct AppState {
    selected_section: usize,
    resume: ResumeData,
    sections: Vec<ResumeSection>,
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

fn build_app_state() -> AppState {
    let source = include_str!("../resume.toml");

    match toml::from_str::<ResumeData>(source) {
        Ok(resume) => AppState {
            selected_section: 0,
            sections: render_sections(&resume),
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
                selected_section: 0,
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
            match key_event.code {
                KeyCode::Down | KeyCode::Char('j') => state.next(),
                KeyCode::Up | KeyCode::Char('k') => state.previous(),
                KeyCode::Home => state.first(),
                KeyCode::End => state.last(),
                KeyCode::Char('d') => {
                    trigger_download_action(&state.resume);
                }
                KeyCode::Char('x') => {
                    state.ui_scale = (state.ui_scale + 0.1).clamp(0.85, 1.5);
                    set_resume_scale(state.ui_scale);
                }
                KeyCode::Char('z') => {
                    state.ui_scale = (state.ui_scale - 0.1).clamp(0.85, 1.5);
                    set_resume_scale(state.ui_scale);
                }
                KeyCode::Char('c') => {
                    state.ui_scale = 1.0;
                    set_resume_scale(state.ui_scale);
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
        let subtitle_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(12),
                Constraint::Length(button_width),
            ])
            .split(header_rows[1]);

        let subtitle = Paragraph::new(state.resume.header.subtitle.clone())
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_PRIMARY));
        frame.render_widget(subtitle, subtitle_row[0]);

        let button = Paragraph::new(button_text)
            .alignment(Alignment::Right)
            .style(download_button_style(is_download_enabled(&state.resume.header)));
        frame.render_widget(button, subtitle_row[1]);

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

        let footer_text = format!(
            "{} | Zoom: z/x (reset c) [{:.0}%]",
            state.resume.footer.hint,
            state.ui_scale * 100.0
        );
        let footer = Paragraph::new(footer_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_MUTED).bg(BG_BASE));
        frame.render_widget(footer, layout[2]);
    });

    Ok(())
}
