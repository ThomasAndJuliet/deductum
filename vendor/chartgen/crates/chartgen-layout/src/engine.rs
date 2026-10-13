//! Replaceable layout boundary for host orchestration and domain-specific layouts.
use chartgen_model::{
    LayoutConstraints, LayoutStep, Measurements, PreparedData, Result, SemanticPlan, Theme,
    TrainedScale,
};

pub trait LayoutEngine {
    fn solve(
        &self,
        plan: &SemanticPlan,
        data: &PreparedData,
        scale: Option<&TrainedScale>,
        theme: &Theme,
        constraints: &LayoutConstraints,
        measurements: &Measurements,
    ) -> Result<LayoutStep>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultLayout;
impl LayoutEngine for DefaultLayout {
    fn solve(
        &self,
        plan: &SemanticPlan,
        data: &PreparedData,
        scale: Option<&TrainedScale>,
        theme: &Theme,
        constraints: &LayoutConstraints,
        measurements: &Measurements,
    ) -> Result<LayoutStep> {
        crate::solve(plan, data, scale, theme, constraints, measurements)
    }
}
