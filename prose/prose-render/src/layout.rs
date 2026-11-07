// SPDX-License-Identifier: GPL-3.0-only

//! Positions of retained document blocks, independent of their document model.
//!
//! Each height includes any space before its block. Changing one height moves
//! every following block without moving or copying its retained layout.

use std::ops::Range;

/// Prefix sums of block heights. Updates and position queries take O(log n);
/// inserting or removing blocks rebuilds the index in O(n).
#[derive(Clone, Debug, Default)]
pub struct HeightIndex {
    heights: Vec<f32>,
    sums: Vec<f64>,
}

fn height(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

impl HeightIndex {
    pub fn new(heights: Vec<f32>) -> Self {
        let heights: Vec<f32> = heights.into_iter().map(height).collect();
        let mut sums = vec![0.0; heights.len() + 1];
        for (index, value) in heights.iter().enumerate() {
            let i = index + 1;
            sums[i] += f64::from(*value);
            let parent = i + (i & i.wrapping_neg());
            if parent < sums.len() {
                sums[parent] += sums[i];
            }
        }
        Self { heights, sums }
    }

    pub fn len(&self) -> usize {
        self.heights.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<f32> {
        self.heights.get(index).copied()
    }

    /// Set a height, returning whether it changed. Invalid heights collapse to
    /// zero rather than poisoning every following position with NaN/infinity.
    pub fn set(&mut self, index: usize, value: f32) -> bool {
        let value = height(value);
        let Some(previous) = self.heights.get_mut(index) else {
            return false;
        };
        let delta = f64::from(value) - f64::from(*previous);
        if delta == 0.0 {
            return false;
        }
        *previous = value;
        let mut i = index + 1;
        while i < self.sums.len() {
            self.sums[i] += delta;
            i += i & i.wrapping_neg();
        }
        true
    }

    /// The top of a block, including its leading space. `len()` is the end.
    pub fn top(&self, index: usize) -> f32 {
        let mut i = index.min(self.heights.len());
        let mut sum = 0.0;
        while i != 0 {
            sum += self.sums[i];
            i -= i & i.wrapping_neg();
        }
        sum as f32
    }

    pub fn total(&self) -> f32 {
        self.top(self.len())
    }

    /// The block containing `y`, clamped above/below the document. Zero-height
    /// blocks are passed over. An empty document has no block.
    pub fn index_at(&self, y: f32) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let y = if y.is_nan() { 0.0 } else { f64::from(y) };
        let mut at = 0;
        let mut sum = 0.0;
        let mut bit = self.len().next_power_of_two();
        while bit != 0 {
            let next = at + bit;
            if next <= self.len() && sum + self.sums[next] <= y {
                at = next;
                sum += self.sums[next];
            }
            bit >>= 1;
        }
        Some(at.min(self.len() - 1))
    }

    /// Candidate blocks intersecting the half-open vertical interval. This
    /// skips document-wide scans in drawing, hit testing and viewport widgets.
    pub fn visible_range(&self, top: f32, bottom: f32) -> Range<usize> {
        let top = top.max(0.0);
        let bottom = bottom.min(self.total());
        if self.is_empty() || !top.is_finite() || bottom <= top {
            return 0..0;
        }
        let first = self.index_at(top).unwrap();
        let last = self.index_at(bottom).unwrap();
        let end = if self.top(last) >= bottom {
            last
        } else {
            last + 1
        };
        first..end.max(first)
    }

    pub fn splice(&mut self, range: Range<usize>, values: Vec<f32>) {
        if range.len() == values.len() {
            for (index, value) in range.zip(values) {
                self.set(index, value);
            }
        } else {
            let mut heights = std::mem::take(&mut self.heights);
            heights.splice(range, values);
            *self = Self::new(heights);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_one_height_moves_later_blocks_without_changing_their_heights() {
        let mut heights = vec![10.0, 0.0, 5.0, 7.0];
        let mut index = HeightIndex::new(heights.clone());
        assert_eq!(index.index_at(10.0), Some(2));
        assert_eq!(index.visible_range(10.0, 15.0), 2..3);
        index.set(0, 20.0);
        heights[0] = 20.0;
        assert_eq!(index.top(3), 25.0);
        assert_eq!(index.get(3), Some(7.0));
        index.splice(1..2, vec![2.0, 3.0]);
        heights.splice(1..2, vec![2.0, 3.0]);
        for i in 0..=heights.len() {
            assert_eq!(index.top(i), heights[..i].iter().sum::<f32>());
        }
    }

    #[test]
    fn viewport_queries_follow_a_flat_reference_after_many_updates() {
        let mut heights: Vec<f32> = (0..2048).map(|i| (i % 11) as f32).collect();
        let mut index = HeightIndex::new(heights.clone());
        let mut seed = 7u64;
        for _ in 0..1000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let i = (seed >> 32) as usize % heights.len();
            let value = (seed % 41) as f32;
            heights[i] = value;
            index.set(i, value);
            let y = (seed % 18000) as f32;
            let expected = heights
                .iter()
                .scan(0.0, |sum, h| {
                    *sum += h;
                    Some(*sum)
                })
                .position(|bottom| y < bottom)
                .unwrap_or(heights.len() - 1);
            assert_eq!(index.index_at(y), Some(expected));
            assert_eq!(index.total(), heights.iter().sum::<f32>());
        }
    }

    #[test]
    fn empty_collapsed_and_invalid_heights_have_finite_positions() {
        let mut index = HeightIndex::default();
        assert_eq!(index.index_at(0.0), None);
        assert_eq!(index.visible_range(0.0, 500.0), 0..0);
        index.splice(0..0, vec![f32::NAN, -1.0, f32::INFINITY, 5.0]);
        assert_eq!(index.total(), 5.0);
        assert_eq!(index.visible_range(-100.0, 5.0), 3..4);
        assert_eq!(index.visible_range(5.0, 10.0), 0..0);
    }
}
