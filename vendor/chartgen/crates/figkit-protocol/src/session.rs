//! Host-driven, serializable measurement negotiation. No callbacks, registry contents or I/O.
use figkit_model::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub context: MeasurementContext,
    pub rounds: usize,
    #[serde(default)]
    pub awaiting: bool,
    pub replies: BTreeMap<RequestId, MeasurementReply>,
}
impl Session {
    pub fn new(context: MeasurementContext) -> Self {
        Self {
            context,
            rounds: 0,
            awaiting: false,
            replies: BTreeMap::new(),
        }
    }
    /// Validate a whole batch transactionally before admitting it. A reply retains the exact request.
    pub fn advance(
        &mut self,
        requests: &[TextRequest],
        replies: &[MeasurementReply],
        limits: &Limits,
    ) -> FigureResult<Vec<TextRequest>> {
        let fail = |kind, message| FigureError::new(kind, message);
        if requests.len() > limits.max_measurement_requests
            || replies.len() > limits.max_measurement_requests
        {
            return Err(fail(
                ErrorKind::BudgetExceeded,
                "measurement request limit exceeded",
            ));
        }
        if self.awaiting && replies.is_empty() {
            return Err(fail(
                ErrorKind::MissingMeasurement,
                "measurement round made no progress",
            ));
        }
        if self.replies.len() > limits.max_measurement_requests
            || self.rounds > limits.max_measurement_rounds
        {
            return Err(fail(
                ErrorKind::BudgetExceeded,
                "session exceeds configured limits",
            ));
        }
        fn valid(r: &TextRequest) -> bool {
            !r.id.0.is_empty()
                && !r.context.id.0.is_empty()
                && !r.typography.font_family.is_empty()
                && r.typography.font_size.is_finite()
                && r.typography.font_size > 0.
                && r.max_width.is_none_or(|w| w.is_finite() && w > 0.)
        }
        fn valid_metrics(m: &BlockMetrics) -> bool {
            m.size.width.is_finite()
                && m.size.height.is_finite()
                && m.size.width >= 0.
                && m.size.height >= 0.
                && m.baseline
                    .is_none_or(|b| b.is_finite() && b >= 0. && b <= m.size.height)
        }
        for (id, reply) in &self.replies {
            if !valid_metrics(&reply.metrics) {
                return Err(fail(
                    ErrorKind::InvalidInput,
                    "invalid cached measurement envelope",
                ));
            }
            if *id != reply.request.id
                || reply.request.context != self.context
                || !valid(&reply.request)
            {
                return Err(fail(
                    ErrorKind::StaleMeasurement,
                    "invalid cached request identity",
                ));
            }
        }
        let mut wanted = BTreeMap::new();
        for r in requests {
            if !valid(r) {
                return Err(fail(ErrorKind::InvalidInput, "invalid measurement request"));
            }
            if r.context != self.context {
                return Err(fail(
                    ErrorKind::StaleMeasurement,
                    "request context differs from session",
                ));
            }
            if let Some(old) = wanted.insert(r.id.clone(), r) {
                if old != r {
                    return Err(fail(
                        ErrorKind::StaleMeasurement,
                        "request ID reused with different identity",
                    ));
                }
            }
            if let Some(old) = self.replies.get(&r.id) {
                if old.request != *r {
                    return Err(fail(
                        ErrorKind::StaleMeasurement,
                        "cached measurement identity differs",
                    ));
                }
            }
        }
        let extra: std::collections::BTreeSet<_> = replies
            .iter()
            .filter(|r| !self.replies.contains_key(&r.request.id))
            .map(|r| &r.request.id)
            .collect();
        if self
            .replies
            .len()
            .checked_add(extra.len())
            .is_none_or(|n| n > limits.max_measurement_requests)
        {
            return Err(fail(
                ErrorKind::BudgetExceeded,
                "session request limit exceeded",
            ));
        }
        let mut next = self.replies.clone();
        for reply in replies {
            if wanted.get(&reply.request.id).copied() != Some(&reply.request)
                || reply.request.context != self.context
            {
                return Err(fail(
                    ErrorKind::StaleMeasurement,
                    "reply does not match requested identity",
                ));
            }
            let m = &reply.metrics;
            if !valid_metrics(m) {
                return Err(fail(
                    ErrorKind::InvalidInput,
                    "invalid measurement envelope",
                ));
            }
            if let Some(old) = next.insert(reply.request.id.clone(), reply.clone()) {
                if old != *reply {
                    return Err(fail(
                        ErrorKind::StaleMeasurement,
                        "conflicting measurement reply",
                    ));
                }
            }
        }
        if next.len() > limits.max_measurement_requests {
            return Err(fail(
                ErrorKind::BudgetExceeded,
                "session request limit exceeded",
            ));
        }
        let pending: Vec<_> = requests
            .iter()
            .filter(|r| !next.contains_key(&r.id))
            .cloned()
            .collect();
        if !replies.is_empty() && next.len() == self.replies.len() && !pending.is_empty() {
            return Err(fail(
                ErrorKind::MissingMeasurement,
                "measurement round made no progress",
            ));
        }
        let mut next_rounds = self.rounds;
        if !replies.is_empty() {
            if next_rounds >= limits.max_measurement_rounds {
                return Err(fail(
                    ErrorKind::BudgetExceeded,
                    "measurement round limit exceeded",
                ));
            }
            next_rounds += 1;
        }
        if !pending.is_empty() && next_rounds >= limits.max_measurement_rounds {
            return Err(fail(
                ErrorKind::BudgetExceeded,
                "measurement round limit exceeded",
            ));
        }
        self.rounds = next_rounds;
        self.awaiting = !pending.is_empty();
        self.replies = next;
        Ok(pending)
    }
}

impl Session {
    pub fn validate_figure(&self, figure: &Figure) -> FigureResult<()> {
        fn nodes(session: &Session, items: &[Primitive]) -> FigureResult<()> {
            for item in items {
                match item {
                    Primitive::Text { placed } => {
                        let text = &placed.text;
                        let reply = session.replies.get(&text.request.id).ok_or_else(|| {
                            FigureError::new(
                                ErrorKind::MissingMeasurement,
                                "scene text has no session measurement",
                            )
                        })?;
                        if reply.request != text.request
                            || reply.metrics != text.metrics
                            || text.request.context != session.context
                        {
                            return Err(FigureError::new(
                                ErrorKind::StaleMeasurement,
                                "scene text differs from measured reply",
                            ));
                        }
                    }
                    Primitive::Group { children, .. } => nodes(session, children)?,
                    _ => {}
                }
            }
            Ok(())
        }
        nodes(self, &figure.primitives)
    }
}
