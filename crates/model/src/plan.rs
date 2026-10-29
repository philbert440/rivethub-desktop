/// Where one step of an agent's plan stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The agent's own key for the step, when its todo tool numbers them.
    pub id: Option<String>,
    pub content: String,
    pub status: StepStatus,
}

/// One item of a list an agent writes with a todo tool such as Grok's `todo_write`; one it
/// cancelled has no status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    pub id: Option<String>,
    pub content: String,
    pub status: Option<StepStatus>,
}

/// A list as an agent's todo tool writes it: the whole list, or with `merge` only the items it
/// changes, matched by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todos {
    pub items: Vec<Todo>,
    pub merge: bool,
}

/// The to-do list an agent keeps while it works, as it last sent it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub steps: Vec<Step>,
}

impl Plan {
    pub fn done(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status == StepStatus::Completed)
            .count()
    }

    pub fn finished(&self) -> bool {
        self.done() == self.steps.len()
    }

    /// What the step the agent keys `id` says.
    pub fn named(&self, id: &str) -> Option<&str> {
        self.steps
            .iter()
            .find(|step| step.id.as_deref() == Some(id))
            .map(|step| step.content.as_str())
    }

    /// The step the agent is on now.
    pub fn current(&self) -> Option<&Step> {
        self.steps
            .iter()
            .find(|step| step.status == StepStatus::InProgress)
    }

    /// Takes in a list the agent wrote with its todo tool; a cancelled item leaves the plan.
    pub fn revise(&mut self, todos: &Todos) {
        if !todos.merge {
            self.steps.clear();
        }
        for todo in &todos.items {
            let known = todo
                .id
                .as_ref()
                .and_then(|id| self.steps.iter().position(|step| step.id.as_ref() == Some(id)));
            match (known, todo.status) {
                (Some(at), None) => {
                    self.steps.remove(at);
                }
                (Some(at), Some(status)) => {
                    let step = &mut self.steps[at];
                    step.status = status;
                    if !todo.content.is_empty() {
                        step.content.clone_from(&todo.content);
                    }
                }
                (None, Some(status)) if !todo.content.is_empty() => self.steps.push(Step {
                    id: todo.id.clone(),
                    content: todo.content.clone(),
                    status,
                }),
                (None, _) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_counts_its_done_steps_and_names_the_one_in_progress() {
        let step = |content: &str, status| Step {
            id: None,
            content: content.to_owned(),
            status,
        };
        let plan = Plan {
            steps: vec![
                step("read", StepStatus::Completed),
                step("change", StepStatus::InProgress),
                step("test", StepStatus::Pending),
            ],
        };
        assert_eq!(plan.done(), 1);
        assert!(!plan.finished());
        assert_eq!(plan.current().map(|step| step.content.as_str()), Some("change"));
        assert!(Plan::default().finished());
    }

    #[test]
    fn a_todo_tool_writes_the_plan_whole_or_by_the_items_it_changes() {
        let todo = |id: &str, content: &str, status| Todo {
            id: Some(id.to_owned()),
            content: content.to_owned(),
            status,
        };
        let mut plan = Plan::default();
        plan.revise(&Todos {
            items: vec![
                todo("1", "date", Some(StepStatus::InProgress)),
                todo("2", "uptime", Some(StepStatus::Pending)),
                todo("3", "whoami", Some(StepStatus::Pending)),
            ],
            merge: false,
        });
        plan.revise(&Todos {
            items: vec![
                todo("1", "", Some(StepStatus::Completed)),
                todo("2", "uptime", Some(StepStatus::InProgress)),
                todo("3", "whoami", None),
            ],
            merge: true,
        });
        let shown: Vec<_> = plan
            .steps
            .iter()
            .map(|step| (step.content.as_str(), step.status))
            .collect();
        assert_eq!(
            shown,
            [
                ("date", StepStatus::Completed),
                ("uptime", StepStatus::InProgress)
            ]
        );
        plan.revise(&Todos {
            items: vec![todo("9", "fresh", Some(StepStatus::Pending))],
            merge: false,
        });
        assert_eq!(plan.steps.len(), 1);
    }
}
