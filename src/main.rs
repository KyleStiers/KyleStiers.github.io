use std::{cell::RefCell, io, rc::Rc};

use ratzilla::event::KeyCode;
use ratzilla::ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Terminal,
};
use ratzilla::{DomBackend, WebRenderer};
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
            }
        }
    }
}

fn main() -> io::Result<()> {
    let backend = DomBackend::new()?;
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
    // Layout: header = 3 rows, nav box border-top = 1 row, items start at row 4.
    // Left nav panel is the first 34% of terminal columns.
    terminal.on_mouse_event({
        let state = state.clone();
        move |mouse_event| {
            use ratzilla::event::MouseEventKind;
            match mouse_event.kind {
                MouseEventKind::SingleClick(_) | MouseEventKind::ButtonDown(_) => {}
                _ => return,
            }
            let state_ref = state.borrow();
            let section_count = state_ref.sections.len();
            drop(state_ref);

            // Row 0-2: header; row 3: nav top-border; rows 4.. nav items
            let header_rows: u16 = 4; // header block (3) + top border (1)
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
        let state = state.borrow();
        let area = frame.area();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(2),
            ])
            .split(area);

        let header = Paragraph::new(format!(
            "{} | {}",
            state.resume.header.title, state.resume.header.subtitle
        ))
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::BOTTOM));
        frame.render_widget(header, layout[0]);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
            .split(layout[1]);

        let nav_items: Vec<ListItem> = state
            .sections
            .iter()
            .enumerate()
            .map(|(index, section)| {
                let prefix = if index == state.selected_section { "> " } else { "  " };
                let style = if index == state.selected_section {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                ListItem::new(format!("{prefix}{}", section.title)).style(style)
            })
            .collect();

        let navigation = List::new(nav_items).block(
            Block::default()
                .title(" Sections ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(navigation, body[0]);

        let content = &state.sections[state.selected_section];
        let details = Paragraph::new(content.body.clone())
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title(format!(" {} ", content.title.clone()))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::LightBlue)),
            );
        frame.render_widget(details, body[1]);

        let footer = Paragraph::new(state.resume.footer.hint.clone())
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(footer, layout[2]);
    });

    Ok(())
}
