use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use mahou::build::load_installed_packages;
use mahou::config::recipe_repo_path;
use mahou::package::Package;
use mahou::repo::load_packages;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::widgets::{List, ListItem, ListState};
use std::collections::HashSet;
use std::io;

struct App {
    packages: Vec<Package>,
    query: String,
    selected: usize,
    installed: HashSet<String>,
}

impl App {
    fn new(mut packages: Vec<Package>, installed: HashSet<String>) -> Self {
        packages.sort_by(|left, right| left.name.cmp(&right.name));

        Self {
            packages,
            query: String::new(),
            selected: 0,
            installed,
        }
    }

    fn move_down(&mut self) {
        let filtered_len = self.filtered_indices().len();

        if filtered_len == 0 {
            self.selected = 0;
            return;
        }

        self.selected = { self.selected + 1 }.min(self.packages.len() - 1);
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();

        if query.is_empty() {
            return (0..self.packages.len()).collect();
        }

        let mut scored: Vec<(usize, i64)> = self
            .packages
            .iter()
            .enumerate()
            .filter_map(|(index, package)| {
                score_package(package, &query).map(|score| (index, score))
            })
            .collect();

        scored.sort_by(|(left_index, left_score), (right_index, right_score)| {
            right_score.cmp(left_score).then_with(|| {
                self.packages[*left_index]
                    .name
                    .cmp(&self.packages[*right_index].name)
            })
        });

        scored.into_iter().map(|(index, _score)| index).collect()
    }

    // fn clamp_selection(&mut self, filtered_len: usize) {
    //     if filtered_len == 0 {
    //         self.selected = 0;
    //     } else if self.selected >= filtered_len {
    //         self.selected = filtered_len - 1;
    //     }
    // }

    fn type_char(&mut self, character: char) {
        self.query.push(character);
        self.selected = 0;
    }

    fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
    }
}

fn main() -> Result<(), String> {
    let repo_path = recipe_repo_path();
    let packages = load_packages(&repo_path)?;
    let installed = load_installed_packages()?
        .into_iter()
        .map(|record| record.name)
        .collect();
    let mut app = App::new(packages, installed);

    enable_raw_mode().map_err(|error| format!("Failed to enable raw mode: {}", error))?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|error| format!("Failed to enter alternate screen: {}", error))?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal =
        Terminal::new(backend).map_err(|error| format!("Failed to create terminal: {}", error))?;

    let result = run_app(&mut terminal, &mut app);

    restore_terminal(&mut terminal)?;

    result
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<(), String> {
    disable_raw_mode().map_err(|error| format!("Failed to disable raw mode: {}", error))?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .map_err(|error| format!("Failed to leave alternate screen: {}", error))?;
    terminal
        .show_cursor()
        .map_err(|error| format!("Failed to show cursor: {}", error))?;

    Ok(())
}

fn draw(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &App) -> Result<(), String> {
    terminal
        .draw(|frame| {
            let filtered = app.filtered_indices();
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Length(5),
                ])
                .split(frame.area());

            let header = Paragraph::new(format!(
                "Search: {} | Matches: {} / {} | Press Esc to quit",
                app.query,
                filtered.len(),
                app.packages.len()
            ))
            .block(Block::default().title("Kakera").borders(Borders::ALL));

            frame.render_widget(header, layout[0]);

            let items: Vec<ListItem> = filtered
                .iter()
                .map(|index| {
                    let package = &app.packages[*index];

                    let marker = if app.installed.contains(&package.name) {
                        "[installed] "
                    } else {
                        ""
                    };

                    ListItem::new(format!(
                        "{}{} {} - {}",
                        marker, package.name, package.version, package.description
                    ))
                })
                .collect();

            let mut state = ListState::default();

            if !items.is_empty() {
                state.select(Some(app.selected));
            }

            let list = List::new(items)
                .block(Block::default().title("Packages").borders(Borders::ALL))
                .highlight_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("> ");

            frame.render_stateful_widget(list, layout[1], &mut state);

            let details = selected_detail(app, &filtered)
                .block(Block::default().title("Details").borders(Borders::ALL));

            frame.render_widget(details, layout[2]);
        })
        .map_err(|error| format!("Failed to draw UI: {}", error))?;

    Ok(())
}

fn selected_detail(app: &App, filtered: &[usize]) -> Paragraph<'static> {
    let Some(index) = filtered.get(app.selected) else {
        return Paragraph::new("No matching packages.");
    };

    let Some(package) = app.packages.get(*index) else {
        return Paragraph::new("No packages found.");
    };

    Paragraph::new(format!(
        "{} {}\n{}\nDeps: {}",
        package.name,
        package.version,
        package.description,
        if package.deps.is_empty() {
            "none".to_string()
        } else {
            package.deps.join(", ")
        }
    ))
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), String> {
    loop {
        draw(terminal, app)?;

        if let Event::Key(key) =
            event::read().map_err(|error| format!("Failed to read input: {}", error))?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Down => app.move_down(),
                KeyCode::Up => app.move_up(),
                KeyCode::Backspace => app.backspace(),
                KeyCode::Char(character) => app.type_char(character),
                _ => {}
            }
        }
    }
}

fn score_package(package: &Package, query: &str) -> Option<i64> {
    let name = package.name.to_lowercase();
    let description = package.description.to_lowercase();

    let mut best = None;

    if let Some(score) = score_text(&name, query, 1000) {
        best = Some(score);
    }

    if let Some(score) = score_text(&description, query, 300) {
        best = Some(best.map_or(score, |current| current.max(score)));
    }

    best
}

fn score_text(text: &str, query: &str, base: i64) -> Option<i64> {
    if text == query {
        return Some(base + 10_000);
    }

    if text.starts_with(query) {
        return Some(base + 5_000 - text.len() as i64);
    }

    if let Some(position) = text.find(query) {
        return Some(base + 3_000 - position as i64);
    }

    fuzzy_subsequence_score(text, query).map(|score| base + score)
}

fn fuzzy_subsequence_score(text: &str, query: &str) -> Option<i64> {
    let mut score = 0_i64;
    let mut last_match = None;
    let mut search_start = 0_usize;

    for query_char in query.chars() {
        let search_slice = &text[search_start..];
        let Some(relative_position) = search_slice.find(query_char) else {
            return None;
        };

        let position = search_start + relative_position;

        if let Some(last) = last_match {
            if position == last + 1 {
                score += 30;
            } else {
                score -= (position - last) as i64;
            }
        } else {
            score -= position as i64;
        }

        score += 100;
        last_match = Some(position);
        search_start = position + query_char.len_utf8();
    }

    Some(score)
}
