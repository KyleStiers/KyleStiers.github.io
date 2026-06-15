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
        .map(|entry| {
            format!(
                "{}\n{}\nMentor: {}",
                entry.title, entry.organization, entry.mentor
            )
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
        },
        Err(err) => {
            let resume = ResumeData {
                header: ResumeHeader {
                    title: "Resume Site Error".to_string(),
                    subtitle: "Failed to parse resume.toml".to_string(),
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
                body: format!(
                    "The application could not parse resume.toml.\n\n{}",
                    err
                ),
            }];

            AppState {
                selected_section: 0,
                resume,
                sections,
                last_cols: 120,
            }
        }
    }
}

fn main() -> io::Result<()> {
    // Use a dynamic font atlas for smoother text rendering than the static default atlas.
    let backend = WebGl2Backend::new_with_options(
        WebGl2BackendOptions::new().font_atlas_config(FontAtlasConfig::dynamic(
            &["Fira Code", "Menlo", "Consolas", "monospace"],
            16.0,
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

    terminal.on_key_event({
        let state = state.clone();
        move |key_event| {
            let mut state = state.borrow_mut();
            match key_event.code {
                KeyCode::Down | KeyCode::Char('j') => state.next(),
                KeyCode::Up | KeyCode::Char('k') => state.previous(),
                KeyCode::Home => state.first(),
                KeyCode::End => state.last(),
                _ => {}
            }
        }
    })?;

    // Click on a nav list item selects that section.
    // Layout: header = 4 rows, nav box border-top = 1 row, items start at row 5.
    // Left nav panel is the first 18% of terminal columns.
    terminal.on_mouse_event({
        let state = state.clone();
        move |mouse_event| {
            use ratzilla::event::MouseEventKind;
            match mouse_event.kind {
                MouseEventKind::SingleClick(_) | MouseEventKind::ButtonDown(_) => {}
                _ => return,
            }

            // Header links row (row index 2): Links: LinkedIn | GitHub | Scholar
            let maybe_link = {
                let st = state.borrow();
                let links_line = LINKS_LINE;
                let line_len = links_line.chars().count() as u16;
                let start_col = st.last_cols.saturating_sub(line_len) / 2;
                let end_col = start_col.saturating_add(line_len);
                // Header occupies rows 0..3 with bottom border at row 4.
                // Accept clicks across the full header band for robust hit-testing.
                let in_links_rows = mouse_event.row <= 4;

                if in_links_rows
                    && mouse_event.col >= start_col
                    && mouse_event.col < end_col
                {
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
            if mouse_event.col >= NAV_WIDTH_COLS {
                return;
            }
            let state_ref = state.borrow();
            let section_count = state_ref.sections.len();
            drop(state_ref);

            // Row 0-3: header area; row 4: nav top-border; rows 5.. nav items
            let header_rows: u16 = 5;
            if mouse_event.row < header_rows {
                return;
            }
            let idx = (mouse_event.row - header_rows) as usize;
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
                Constraint::Length(4),
                Constraint::Min(8),
                Constraint::Length(1),
            ])
            .split(area);

        let header = Paragraph::new(vec![
            Line::from(state.resume.header.title.clone()),
            Line::from(state.resume.header.subtitle.clone()),
            Line::from(vec![
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
            ]),
        ])
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(ACCENT_BLUE)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .style(Style::default().bg(BG_BASE))
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(SURFACE_ALT)),
            );
        frame.render_widget(header, layout[0]);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            // Keep navigation readable (fixed width) while letting content use the rest.
            .constraints([
                Constraint::Length(NAV_WIDTH_COLS),
                Constraint::Min(20),
            ])
            .split(layout[1]);

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

        let footer = Paragraph::new(state.resume.footer.hint.clone())
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_MUTED).bg(BG_BASE));
        frame.render_widget(footer, layout[2]);
    });

    Ok(())
}
