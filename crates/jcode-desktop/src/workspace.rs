#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputMode {
    Navigation,
    Insert,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyInput {
    Escape,
    Enter,
    Backspace,
    Character(String),
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyOutcome {
    None,
    Redraw,
    Exit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Surface {
    pub id: u64,
    pub title: String,
    pub lane: i32,
    pub column: i32,
    pub color_index: usize,
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub mode: InputMode,
    pub surfaces: Vec<Surface>,
    pub focused_id: u64,
    pub zoomed: bool,
    pub draft: String,
    next_id: u64,
}

impl Workspace {
    pub fn fake() -> Self {
        let surfaces = vec![
            Surface {
                id: 1,
                title: "fox · coordinator".to_string(),
                lane: 0,
                column: 0,
                color_index: 0,
            },
            Surface {
                id: 2,
                title: "wolf · impl".to_string(),
                lane: 0,
                column: 1,
                color_index: 1,
            },
            Surface {
                id: 3,
                title: "owl · review".to_string(),
                lane: 0,
                column: 2,
                color_index: 2,
            },
            Surface {
                id: 4,
                title: "activity".to_string(),
                lane: 1,
                column: 0,
                color_index: 3,
            },
            Surface {
                id: 5,
                title: "diff".to_string(),
                lane: 1,
                column: 1,
                color_index: 4,
            },
        ];

        Self {
            mode: InputMode::Navigation,
            surfaces,
            focused_id: 1,
            zoomed: false,
            draft: String::new(),
            next_id: 6,
        }
    }

    pub fn status_title(&self) -> String {
        let mode = match self.mode {
            InputMode::Navigation => "NAV",
            InputMode::Insert => "INSERT",
        };
        let zoom = if self.zoomed { " · ZOOM" } else { "" };
        let focused = self
            .focused_surface()
            .map(|surface| surface.title.as_str())
            .unwrap_or("no surface");

        match self.mode {
            InputMode::Navigation => format!(
                "Jcode Desktop · {mode}{zoom} · {focused} · h/j/k/l focus · H/J/K/L move · n new · z zoom · i insert · Esc quit"
            ),
            InputMode::Insert => {
                format!("Jcode Desktop · {mode}{zoom} · {focused} · typing captured · Esc NAV")
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyInput) -> KeyOutcome {
        match self.mode {
            InputMode::Navigation => self.handle_navigation_key(key),
            InputMode::Insert => self.handle_insert_key(key),
        }
    }

    pub fn focused_surface(&self) -> Option<&Surface> {
        self.surfaces
            .iter()
            .find(|surface| surface.id == self.focused_id)
    }

    pub fn is_focused(&self, surface_id: u64) -> bool {
        self.focused_id == surface_id
    }

    fn handle_navigation_key(&mut self, key: KeyInput) -> KeyOutcome {
        let KeyInput::Character(text) = key else {
            return match key {
                KeyInput::Escape => KeyOutcome::Exit,
                KeyInput::Enter => {
                    self.mode = InputMode::Insert;
                    KeyOutcome::Redraw
                }
                _ => KeyOutcome::None,
            };
        };

        match text.as_str() {
            "h" => self.focus(Direction::Left),
            "j" => self.focus(Direction::Down),
            "k" => self.focus(Direction::Up),
            "l" => self.focus(Direction::Right),
            "H" => self.move_focused(Direction::Left),
            "J" => self.move_focused(Direction::Down),
            "K" => self.move_focused(Direction::Up),
            "L" => self.move_focused(Direction::Right),
            "i" => {
                self.mode = InputMode::Insert;
                true
            }
            "n" => {
                self.add_surface();
                true
            }
            "x" => self.close_focused(),
            "z" => {
                self.zoomed = !self.zoomed;
                true
            }
            _ => false,
        }
        .into()
    }

    fn handle_insert_key(&mut self, key: KeyInput) -> KeyOutcome {
        match key {
            KeyInput::Escape => {
                self.mode = InputMode::Navigation;
                KeyOutcome::Redraw
            }
            KeyInput::Enter => {
                self.draft.push('\n');
                KeyOutcome::Redraw
            }
            KeyInput::Backspace => {
                self.draft.pop();
                KeyOutcome::Redraw
            }
            KeyInput::Character(text) => {
                self.draft.push_str(&text);
                KeyOutcome::Redraw
            }
            KeyInput::Other => KeyOutcome::None,
        }
    }

    fn focus(&mut self, direction: Direction) -> bool {
        if let Some(next_id) = self.neighbor_id(direction) {
            self.focused_id = next_id;
            true
        } else {
            false
        }
    }

    fn neighbor_id(&self, direction: Direction) -> Option<u64> {
        let current = self.focused_surface()?;
        let current_lane = current.lane;
        let current_column = current.column;

        self.surfaces
            .iter()
            .filter(|surface| match direction {
                Direction::Left => surface.column < current_column,
                Direction::Right => surface.column > current_column,
                Direction::Up => surface.lane < current_lane,
                Direction::Down => surface.lane > current_lane,
            })
            .min_by_key(|surface| match direction {
                Direction::Left | Direction::Right => (
                    (surface.column - current_column).abs(),
                    (surface.lane - current_lane).abs(),
                    surface.id,
                ),
                Direction::Up | Direction::Down => (
                    (surface.lane - current_lane).abs(),
                    (surface.column - current_column).abs(),
                    surface.id,
                ),
            })
            .map(|surface| surface.id)
    }

    fn move_focused(&mut self, direction: Direction) -> bool {
        let Some(focused_index) = self
            .surfaces
            .iter()
            .position(|surface| surface.id == self.focused_id)
        else {
            return false;
        };

        if let Some(neighbor_id) = self.neighbor_id(direction) {
            if let Some(neighbor_index) = self
                .surfaces
                .iter()
                .position(|surface| surface.id == neighbor_id)
            {
                let focused_position = (
                    self.surfaces[focused_index].lane,
                    self.surfaces[focused_index].column,
                );
                let neighbor_position = (
                    self.surfaces[neighbor_index].lane,
                    self.surfaces[neighbor_index].column,
                );
                self.surfaces[focused_index].lane = neighbor_position.0;
                self.surfaces[focused_index].column = neighbor_position.1;
                self.surfaces[neighbor_index].lane = focused_position.0;
                self.surfaces[neighbor_index].column = focused_position.1;
                return true;
            }
        }

        let surface = &mut self.surfaces[focused_index];
        match direction {
            Direction::Left => surface.column -= 1,
            Direction::Right => surface.column += 1,
            Direction::Up => surface.lane -= 1,
            Direction::Down => surface.lane += 1,
        }
        true
    }

    fn add_surface(&mut self) {
        let lane = self
            .focused_surface()
            .map(|surface| surface.lane)
            .unwrap_or_default();
        let column = self
            .surfaces
            .iter()
            .filter(|surface| surface.lane == lane)
            .map(|surface| surface.column)
            .max()
            .unwrap_or(-1)
            + 1;
        let id = self.next_id;
        self.next_id += 1;
        self.surfaces.push(Surface {
            id,
            title: format!("agent-{id}"),
            lane,
            column,
            color_index: id as usize,
        });
        self.focused_id = id;
        self.zoomed = false;
    }

    fn close_focused(&mut self) -> bool {
        if self.surfaces.len() <= 1 {
            return false;
        }
        let Some(position) = self
            .surfaces
            .iter()
            .position(|surface| surface.id == self.focused_id)
        else {
            return false;
        };
        self.surfaces.remove(position);
        let new_position = position.min(self.surfaces.len() - 1);
        self.focused_id = self.surfaces[new_position].id;
        self.zoomed = false;
        true
    }
}

impl From<bool> for KeyOutcome {
    fn from(value: bool) -> Self {
        if value { Self::Redraw } else { Self::None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn hjkl_focuses_neighboring_surfaces() {
        let mut workspace = Workspace::fake();
        assert_eq!(workspace.focused_id, 1);
        assert_eq!(
            workspace.handle_key(KeyInput::Character("l".to_string())),
            KeyOutcome::Redraw
        );
        assert_eq!(workspace.focused_id, 2);
        assert_eq!(
            workspace.handle_key(KeyInput::Character("j".to_string())),
            KeyOutcome::Redraw
        );
        assert_eq!(workspace.focused_id, 5);
        assert_eq!(
            workspace.handle_key(KeyInput::Character("h".to_string())),
            KeyOutcome::Redraw
        );
        assert_eq!(workspace.focused_id, 4);
    }

    #[test]
    fn uppercase_hjkl_swaps_focused_surface_with_neighbor() {
        let mut workspace = Workspace::fake();
        workspace.handle_key(KeyInput::Character("L".to_string()));
        assert_eq!(
            workspace
                .focused_surface()
                .map(|surface| (surface.lane, surface.column)),
            Some((0, 1))
        );
        workspace.handle_key(KeyInput::Character("J".to_string()));
        assert_eq!(
            workspace
                .focused_surface()
                .map(|surface| (surface.lane, surface.column)),
            Some((1, 1))
        );
        assert_unique_positions(&workspace);
    }

    #[test]
    fn insert_mode_captures_text_and_escape_returns_to_navigation() {
        let mut workspace = Workspace::fake();
        assert_eq!(
            workspace.handle_key(KeyInput::Character("i".to_string())),
            KeyOutcome::Redraw
        );
        assert_eq!(workspace.mode, InputMode::Insert);
        workspace.handle_key(KeyInput::Character("hello".to_string()));
        assert_eq!(workspace.draft, "hello");
        workspace.handle_key(KeyInput::Escape);
        assert_eq!(workspace.mode, InputMode::Navigation);
    }

    #[test]
    fn navigation_escape_exits() {
        let mut workspace = Workspace::fake();
        assert_eq!(workspace.handle_key(KeyInput::Escape), KeyOutcome::Exit);
    }

    #[test]
    fn new_and_close_surface_update_focus_without_overlapping() {
        let mut workspace = Workspace::fake();
        workspace.handle_key(KeyInput::Character("n".to_string()));
        assert_eq!(workspace.focused_id, 6);
        assert_eq!(workspace.surfaces.len(), 6);
        assert_unique_positions(&workspace);
        workspace.handle_key(KeyInput::Character("x".to_string()));
        assert_eq!(workspace.surfaces.len(), 5);
        assert_ne!(workspace.focused_id, 6);
    }

    fn assert_unique_positions(workspace: &Workspace) {
        let positions: HashSet<(i32, i32)> = workspace
            .surfaces
            .iter()
            .map(|surface| (surface.lane, surface.column))
            .collect();
        assert_eq!(positions.len(), workspace.surfaces.len());
    }
}
