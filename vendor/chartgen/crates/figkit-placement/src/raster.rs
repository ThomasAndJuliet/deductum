//! One-point occupancy cells and summed-area queries with checked allocation and work.
use crate::*;
#[derive(Debug)]
pub struct Occupancy {
    nx: usize,
    ny: usize,
    cells: Vec<u8>,
    sat: Option<Vec<i64>>,
    profile: Profile,
    budget: Budget,
}
impl Occupancy {
    pub fn new(w: f64, h: f64, profile: Profile, budget: &Budget) -> Result<Self> {
        finite(&[w, h])?;
        if w < 0.
            || h < 0.
            || w.ceil() >= (usize::MAX - 2) as f64
            || h.ceil() >= (usize::MAX - 2) as f64
        {
            return Err(exhausted("cells"));
        }
        let nx = (w.ceil() as usize)
            .checked_add(2)
            .ok_or_else(|| exhausted("cells"))?;
        let ny = (h.ceil() as usize)
            .checked_add(2)
            .ok_or_else(|| exhausted("cells"))?;
        let n = nx.checked_mul(ny).ok_or_else(|| exhausted("cells"))?;
        budget.charge("cells", n)?;
        Ok(Self {
            nx,
            ny,
            cells: vec![0; n],
            sat: None,
            profile,
            budget: budget.clone(),
        })
    }
    pub fn budget(&self) -> &Budget {
        &self.budget
    }
    pub fn try_clone(&self) -> Result<Self> {
        let count = self
            .cells
            .len()
            .checked_add(self.sat.as_ref().map_or(0, Vec::len))
            .ok_or_else(|| exhausted("cells"))?;
        self.budget.charge("cells", count)?;
        Ok(Self {
            nx: self.nx,
            ny: self.ny,
            cells: self.cells.clone(),
            sat: self.sat.clone(),
            profile: self.profile,
            budget: self.budget.clone(),
        })
    }
    pub fn rect(&mut self, b: Box2) -> Result<()> {
        valid_box(b)?;
        let x0 = b[0].floor().max(0.) as i64;
        let y0 = b[1].floor().max(0.) as i64;
        let x1 = (b[2].ceil().max(i64::MIN as f64 + 1024.) as i64)
            .saturating_sub(1)
            .min(self.nx as i64 - 1);
        let y1 = (b[3].ceil().max(i64::MIN as f64 + 1024.) as i64)
            .saturating_sub(1)
            .min(self.ny as i64 - 1);
        if x1 < x0 || y1 < y0 {
            return Ok(());
        }
        let work = (x1 - x0 + 1)
            .checked_mul(y1 - y0 + 1)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| exhausted("raster"))?;
        self.budget.charge("raster", work)?;
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.cells[y as usize * self.nx + x as usize] = 1;
            }
        }
        self.sat = None;
        Ok(())
    }
    pub fn segment(&mut self, p: Position, q: Position, pad: f64) -> Result<()> {
        finite(&[p.0, p.1, q.0, q.1, pad])?;
        if pad < 0. {
            return Err(invalid("negative segment clearance"));
        }
        let distance = match self.profile {
            Profile::LegacyFamily => (p.0 - q.0).hypot(p.1 - q.1),
            Profile::LegacySpecial => ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt(),
        };
        if !distance.is_finite() || distance / 0.5 >= usize::MAX as f64 {
            return Err(exhausted("raster"));
        }
        let n = (distance / 0.5).max(1.) as usize;
        self.budget.charge(
            "raster",
            n.checked_add(1).ok_or_else(|| exhausted("raster"))?,
        )?;
        for k in 0..=n {
            let (x, y) = match self.profile {
                Profile::LegacyFamily => {
                    let t = k as f64 / n as f64;
                    (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t)
                }
                Profile::LegacySpecial => (
                    p.0 + (q.0 - p.0) * k as f64 / n as f64,
                    p.1 + (q.1 - p.1) * k as f64 / n as f64,
                ),
            };
            self.rect([x - pad, y - pad, x + pad, y + pad])?;
        }
        Ok(())
    }
    fn build(&mut self) -> Result<()> {
        let w = self.nx.checked_add(1).ok_or_else(|| exhausted("cells"))?;
        let h = self.ny.checked_add(1).ok_or_else(|| exhausted("cells"))?;
        let n = w.checked_mul(h).ok_or_else(|| exhausted("cells"))?;
        self.budget.charge("cells", n)?;
        self.budget.charge("raster", self.cells.len())?;
        let mut sat = vec![0i64; n];
        for y in 0..self.ny {
            let mut acc = 0;
            for x in 0..self.nx {
                acc += self.cells[y * self.nx + x] as i64;
                sat[(y + 1) * w + x + 1] = sat[y * w + x + 1] + acc;
            }
        }
        self.sat = Some(sat);
        Ok(())
    }
    pub fn free(&mut self, b: Box2) -> Result<bool> {
        valid_box(b)?;
        if self.sat.is_none() {
            self.build()?;
        }
        let x0 = b[0].floor() as i64;
        let y0 = b[1].floor() as i64;
        let x1 = b[2].ceil() as i64;
        let y1 = b[3].ceil() as i64;
        if x0 < 0 || y0 < 0 || x1 > self.nx as i64 || y1 > self.ny as i64 {
            return Ok(false);
        }
        let x1 = x1.max(x0.saturating_add(1)) as usize;
        let y1 = y1.max(y0.saturating_add(1)) as usize;
        if x1 > self.nx || y1 > self.ny {
            return Ok(false);
        }
        let (x0, y0) = (x0 as usize, y0 as usize);
        let w = self.nx + 1;
        let s = self.sat.as_ref().unwrap();
        Ok(s[y1 * w + x1] - s[y0 * w + x1] - s[y1 * w + x0] + s[y0 * w + x0] == 0)
    }
    pub fn at(&self, x: f64, y: f64) -> bool {
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        if self.profile == Profile::LegacyFamily && (x < 0. || y < 0.) {
            return false;
        }
        let (i, j) = (x as i64, y as i64);
        i >= 0
            && j >= 0
            && (i as usize) < self.nx
            && (j as usize) < self.ny
            && self.cells[j as usize * self.nx + i as usize] != 0
    }
}
