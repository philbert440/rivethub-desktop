/// Where one step of an agent's plan stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub content: String,
    pub status: StepStatus,
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

    /// The step the agent is on now.
    pub fn current(&self) -> Option<&Step> {
        self.steps
            .iter()
            .find(|step| step.status == StepStatus::InProgress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_counts_its_done_steps_and_names_the_one_in_progress() {
        let step = |content: &str, status| Step {
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
}
