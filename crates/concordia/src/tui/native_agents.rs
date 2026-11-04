//! Read-only access to child-agent state reported by the native provider.
use super::*;

pub(super) fn panel_area(area: Rect) -> Rect {
    if area.width < 96 {
        return area;
    }
    let width = (area.width / 2).clamp(48, 86);
    Rect {
        x: area.x + area.width - width,
        width,
        ..area
    }
}

#[derive(Default)]
pub(super) struct State {
    cards: Vec<(String, usize)>,
    launch_tools: Vec<String>,
}

impl State {
    pub fn record(&mut self, card: &StreamCard, index: usize) {
        let id = &card.id;
        if let Some(identity) = id.strip_prefix("native-agent:claude:") {
            if let Ok((_, _, Some(tool))) =
                serde_json::from_str::<(String, String, Option<String>)>(identity)
            {
                if !self.launch_tools.contains(&tool) {
                    if self.launch_tools.len() >= 64 {
                        self.launch_tools.remove(0);
                    }
                    self.launch_tools.push(tool.clone());
                }
                self.cards.retain(|(old, _)| old != &tool);
            }
        }
        if self.launch_tools.contains(id) {
            return;
        }
        if !id.starts_with("native-agent:") && !matches!(card.title.as_str(), "Agent" | "Task") {
            return;
        }
        if let Some((_, old)) = self.cards.iter_mut().find(|(old, _)| old == id) {
            *old = index;
        } else {
            if self.cards.len() >= 64 {
                self.cards.remove(0);
            }
            self.cards.push((id.into(), index));
        }
    }
    pub fn count(&self) -> usize {
        self.cards.len()
    }
}

impl App {
    pub(super) fn open_agents_picker(&mut self) {
        let mut rows: Vec<_> = self
            .native_agents
            .cards
            .iter()
            .rev()
            .filter_map(|(_, index)| {
                let card = self.messages.get(*index)?.card.as_ref()?;
                Some(PickerRow {
                    id: format!("native-agent:{index}"),
                    label: format!(
                        "{} · {}\n{}",
                        card.title,
                        match card.status {
                            CardStatus::Running => "running",
                            CardStatus::Done => "finished",
                            CardStatus::Error => "failed/interrupted",
                        },
                        card.detail
                            .as_deref()
                            .unwrap_or("")
                            .lines()
                            .find_map(|line| line.strip_prefix("Task: "))
                            .unwrap_or("Native agent task")
                    ),
                    detail: card
                        .detail
                        .as_deref()
                        .unwrap_or("")
                        .lines()
                        .filter(|line| !line.starts_with("Task: "))
                        .take(4)
                        .collect::<Vec<_>>()
                        .join(" · "),
                })
            })
            .collect();
        rows.extend(
            self.deleg
                .d
                .visible(Instant::now(), Duration::MAX)
                .into_iter()
                .take(64)
                .map(|job| PickerRow {
                    id: format!("worker-agent:{}", job.id),
                    label: format!(
                        "#{} {} · {} · {}\n{}",
                        job.id,
                        job.label(),
                        job.model.as_deref().unwrap_or("default"),
                        job.state.label(),
                        job.task
                            .lines()
                            .next()
                            .unwrap_or("")
                            .chars()
                            .take(160)
                            .collect::<String>()
                    ),
                    detail: format!(
                        "{} · {}",
                        job.cards
                            .last()
                            .map(crate::provider::stream_cards::format_card_line)
                            .unwrap_or_else(|| job.state.label().into()),
                        if job.collected {
                            "Result returned to main agent"
                        } else if job.state.is_running() {
                            "Work in progress"
                        } else {
                            "Result available to inspect"
                        }
                    ),
                }),
        );
        if rows.is_empty() {
            self.push_system("No native subagents have been reported in this conversation. Cross-account workers are managed with /workers; /delegate ACCOUNT TASK starts one.");
            return;
        }
        self.show_picker(
            PickerKind::Agents,
            "Agents · Enter inspect · Esc close".into(),
            rows,
            true,
        );
    }

    pub(super) fn refresh_agents_picker(&mut self) {
        let Some(picker) = self
            .picker
            .as_ref()
            .filter(|picker| picker.kind == PickerKind::Agents)
        else {
            return;
        };
        let filter = picker.filter.clone();
        let selected = picker
            .state
            .selected()
            .and_then(|index| picker.filtered().get(index).map(|row| row.id.clone()));
        self.open_agents_picker();
        if let Some(picker) = self.picker.as_mut() {
            picker.filter = filter;
            let index = selected
                .and_then(|id| picker.filtered().iter().position(|row| row.id == id))
                .unwrap_or(0);
            picker.state.select(Some(index));
            picker.select_clamp();
        }
    }
}
