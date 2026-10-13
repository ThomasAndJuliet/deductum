//! Read-only output checks under explicit assumptions; these do not certify readability.
use figkit_model::ElementId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputProfile {
    pub name: String,
    /// Opaque solid background. Unknown colours produce indeterminate checks.
    pub background: String,
    pub output_scale: f64,
    pub minimum_contrast: f64,
    pub minimum_stroke_pt: f64,
    /// Minimum absolute difference in linear-sRGB relative luminance.
    pub minimum_monochrome_difference: f64,
}
impl Default for OutputProfile {
    fn default() -> Self {
        Self {
            name: "monochrome-print".into(),
            background: "#ffffff".into(),
            output_scale: 1.,
            minimum_contrast: 4.5,
            minimum_stroke_pt: 0.25,
            minimum_monochrome_difference: 0.1,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileElement {
    pub element: ElementId,
    pub paint: String,
    pub opacity: f64,
    pub stroke_width: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredDistinction {
    pub first: ElementId,
    pub second: ElementId,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileObservation {
    LowContrast {
        element: ElementId,
        ratio: f64,
    },
    ThinStroke {
        element: ElementId,
        width_pt: f64,
    },
    LostDistinction {
        first: ElementId,
        second: ElementId,
        difference: f64,
    },
    Indeterminate {
        elements: Vec<ElementId>,
        reason: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileReport {
    pub assumptions: OutputProfile,
    pub provenance: String,
    pub observations: Vec<ProfileObservation>,
}
/// Supported colours are #rgb/#rrggbb and black/white/red/green/blue/gray/grey.
/// Conversion: sRGB transfer curve, then Y = .2126 R + .7152 G + .0722 B.
/// This is a specified screening assumption, not a printer or paper model.
pub fn relative_luminance(paint: &str) -> Option<f64> {
    let normalized = paint.trim().to_ascii_lowercase();
    let hex = match normalized.as_str() {
        "black" => "000000",
        "white" => "ffffff",
        "red" => "ff0000",
        "green" => "008000",
        "blue" => "0000ff",
        "gray" | "grey" => "808080",
        other => other.strip_prefix('#')?,
    };
    let expanded;
    let hex = if hex.len() == 3 && hex.is_ascii() {
        expanded = hex.chars().flat_map(|c| [c, c]).collect::<String>();
        &expanded
    } else {
        hex
    };
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |offset| {
        let value = u8::from_str_radix(&hex[offset..offset + 2], 16).ok()? as f64 / 255.;
        Some(figkit_colour::from_gamma(value))
    };
    Some(0.2126 * channel(0)? + 0.7152 * channel(2)? + 0.0722 * channel(4)?)
}
/// Check only caller-supplied obligations; authored paint and geometry are untouched.
pub fn check(
    profile: &OutputProfile,
    elements: &[ProfileElement],
    distinctions: &[RequiredDistinction],
) -> ProfileReport {
    let mut observations = Vec::new();
    let valid = profile.output_scale.is_finite()
        && profile.output_scale > 0.
        && profile.minimum_contrast.is_finite()
        && profile.minimum_contrast >= 1.
        && profile.minimum_stroke_pt.is_finite()
        && profile.minimum_stroke_pt >= 0.
        && profile.minimum_monochrome_difference.is_finite()
        && (0. ..=1.).contains(&profile.minimum_monochrome_difference);
    if !valid {
        observations.push(ProfileObservation::Indeterminate {
            elements: vec![],
            reason: "invalid profile assumptions".into(),
        });
    } else {
        let background = relative_luminance(&profile.background);
        for element in elements {
            match (
                relative_luminance(&element.paint).filter(|_| element.opacity == 1.),
                background,
            ) {
                (Some(ink), Some(background)) => {
                    let ratio = (ink.max(background) + 0.05) / (ink.min(background) + 0.05);
                    if ratio < profile.minimum_contrast {
                        observations.push(ProfileObservation::LowContrast {
                            element: element.element.clone(),
                            ratio,
                        });
                    }
                }
                _ => observations.push(ProfileObservation::Indeterminate {
                    elements: vec![element.element.clone()],
                    reason: "contrast requires supported opaque solid paint and background".into(),
                }),
            }
            if let Some(width) = element.stroke_width {
                let width_pt = width * profile.output_scale;
                if !width_pt.is_finite() || width_pt < 0. {
                    observations.push(ProfileObservation::Indeterminate {
                        elements: vec![element.element.clone()],
                        reason: "invalid stroke width at output scale".into(),
                    });
                } else if width_pt < profile.minimum_stroke_pt {
                    observations.push(ProfileObservation::ThinStroke {
                        element: element.element.clone(),
                        width_pt,
                    });
                }
            }
        }
        for distinction in distinctions {
            let find = |id: &ElementId| {
                let mut matches = elements.iter().filter(|e| &e.element == id);
                let element = matches.next()?;
                if matches.next().is_some() || element.opacity != 1. {
                    return None;
                }
                relative_luminance(&element.paint)
            };
            match (find(&distinction.first), find(&distinction.second)) {
                (Some(a), Some(b)) => {
                    let difference = (a - b).abs();
                    if difference < profile.minimum_monochrome_difference {
                        observations.push(ProfileObservation::LostDistinction {
                            first: distinction.first.clone(),
                            second: distinction.second.clone(),
                            difference,
                        });
                    }
                }
                _ => observations.push(ProfileObservation::Indeterminate {
                    elements: vec![distinction.first.clone(), distinction.second.clone()],
                    reason:
                        "distinction requires unique supplied elements with supported opaque paints"
                            .into(),
                }),
            }
        }
    }
    ProfileReport { assumptions: profile.clone(), provenance: "figkit monochrome-print v1: linear sRGB relative luminance; contrast (lighter Y + 0.05)/(darker Y + 0.05); physical stroke width times output scale".into(), observations }
}
