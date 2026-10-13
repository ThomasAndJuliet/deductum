//! Colour-space maths of the original figkit (style.py) and okhsl.py: sRGB, OKLab, OKLCH with
//! gamut reduction, OKHSL, WCAG contrast, mixing, text tone on fills, and colour-vision-deficiency
//! simulation (svglint.CVD). A leaf crate: no other chartgen dependency.
//!
//! Python's arithmetic is followed exactly: `**` is `powf`, `min`/`max` keep the first argument
//! unless the second compares strictly beyond it (so NaN never wins), `round` halves to even, and
//! `hex_rgb` reads each two-character slice as `int(…, 16)` does (signs and padding accepted).

/// Python's `str.isspace` for one character (what `str.strip()` removes).
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}
/// `max(a, b)`: the first unless the second is strictly greater (so NaN never wins).
pub fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}
/// `min(a, b)`: the first unless the second is strictly less.
pub fn py_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

pub fn hex_slice(s: &str) -> Option<i64> {
    let t = s.trim_matches(is_space);
    let (neg, digits) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = i64::from_str_radix(digits, 16).ok()?;
    Some(if neg { -v } else { v })
}

/// The slices the original's `hex_rgb` reads: the code without surrounding whitespace and
/// leading `#`s, a 3-character code doubled, then characters [0:2], [2:4], [4:6].
pub fn hex_slices(h: &str) -> [String; 3] {
    let h = h.trim_matches(is_space).trim_start_matches('#');
    let chars: Vec<char> = if h.chars().count() == 3 {
        h.chars().flat_map(|c| [c, c]).collect()
    } else {
        h.chars().collect()
    };
    let slice = |i: usize| chars.iter().skip(i).take(2).collect::<String>();
    [slice(0), slice(2), slice(4)]
}

/// (r, g, b) in 0…1 of a #rgb or #rrggbb colour; None where the original raises ValueError.
pub fn hex_rgb(h: &str) -> Option<[f64; 3]> {
    let s = hex_slices(h);
    Some([
        hex_slice(&s[0])? as f64 / 255.,
        hex_slice(&s[1])? as f64 / 255.,
        hex_slice(&s[2])? as f64 / 255.,
    ])
}

fn round_even(x: f64) -> f64 {
    x.round_ties_even()
}

/// `#rrggbb` of (r, g, b), each clamped to 0…1 and rounded half to even.
pub fn rgb_hex(rgb: [f64; 3]) -> String {
    let c = |v: f64| round_even(py_min(1., py_max(0., v)) * 255.) as u8;
    format!("#{:02x}{:02x}{:02x}", c(rgb[0]), c(rgb[1]), c(rgb[2]))
}

pub fn to_gamma(x: f64) -> f64 {
    let x = py_min(py_max(x, 0.0), 1.0);
    if x <= 0.0031308 {
        12.92 * x
    } else {
        1.055 * x.powf(1. / 2.4) - 0.055
    }
}
pub fn from_gamma(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

pub fn oklab_to_linear_srgb(l: f64, a: f64, b: f64) -> [f64; 3] {
    let l_ = l + 0.3963377774 * a + 0.2158037573 * b;
    let m_ = l - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = l - 0.0894841775 * a - 1.2914855480 * b;
    let (l, m, s) = (l_.powf(3.), m_.powf(3.), s_.powf(3.));
    [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ]
}

pub fn linear_srgb_to_oklab(r: f64, g: f64, b: f64) -> [f64; 3] {
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let cbrt = |v: f64| v.abs().powf(1. / 3.).copysign(v);
    let (l_, m_, s_) = (cbrt(l), cbrt(m), cbrt(s));
    [
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    ]
}

fn radians(x: f64) -> f64 {
    x * (std::f64::consts::PI / 180.0)
}
fn degrees(x: f64) -> f64 {
    x * (180.0 / std::f64::consts::PI)
}
/// Python's float `x % m` for m > 0.
fn py_mod(x: f64, m: f64) -> f64 {
    let r = x % m;
    if r != 0. && r < 0. {
        r + m
    } else if r == 0. {
        0.
    } else {
        r
    }
}

/// (L, C, hue in degrees) of a hex colour.
pub fn oklch(h: &str) -> Option<[f64; 3]> {
    let [r, g, b] = hex_rgb(h)?.map(from_gamma);
    let [l, a, bb] = linear_srgb_to_oklab(r, g, b);
    Some([l, a.hypot(bb), py_mod(degrees(bb.atan2(a)), 360.)])
}

/// A hex colour from OKLCH, its chroma reduced (by 5 % a step, at most 40 steps) until inside sRGB.
pub fn from_oklch(l: f64, c: f64, hue: f64) -> String {
    let mut c = c;
    let mut lin = [0.; 3];
    for _ in 0..40 {
        let (a, b) = (c * radians(hue).cos(), c * radians(hue).sin());
        lin = oklab_to_linear_srgb(l, a, b);
        if lin.iter().all(|&v| -1e-6 <= v && v <= 1. + 1e-6) {
            break;
        }
        c *= 0.95;
    }
    rgb_hex(lin.map(|v| to_gamma(py_min(1., py_max(0., v)))))
}

/// t of the way from a to b, in sRGB.
pub fn mix(a: &str, b: &str, t: f64) -> Option<String> {
    let (ca, cb) = (hex_rgb(a)?, hex_rgb(b)?);
    Some(rgb_hex([0, 1, 2].map(|i| ca[i] + (cb[i] - ca[i]) * t)))
}

/// Lightness and chroma moved in OKLCH, the hue kept.
pub fn oklch_shift(c: &str, dl: f64, dc: f64) -> Option<String> {
    let [l, cc, h] = oklch(c)?;
    Some(from_oklch(
        py_min(1., py_max(0., l + dl)),
        py_max(0., cc + dc),
        h,
    ))
}

/// c at OKLCH lightness l, its hue kept and its chroma at most cmax.
pub fn oklch_light(c: &str, l: f64, cmax: Option<f64>) -> Option<String> {
    let [_, cc, h] = oklch(c)?;
    Some(from_oklch(
        py_min(1., py_max(0., l)),
        cmax.map_or(cc, |m| py_min(cc, m)),
        h,
    ))
}

/// WCAG relative luminance.
pub fn luminance(h: &str) -> Option<f64> {
    let [r, g, b] = hex_rgb(h)?.map(from_gamma);
    Some(0.2126 * r + 0.7152 * g + 0.0722 * b)
}

/// WCAG contrast of two colours.
pub fn contrast(a: &str, b: &str) -> Option<f64> {
    let (la, lb) = (luminance(a)?, luminance(b)?);
    let (hi, lo) = if lb > la { (lb, la) } else { (la, lb) };
    Some((hi + 0.05) / (lo + 0.05))
}

/// ("dark" or "light", its contrast): the text colour with more contrast on every fill, judged
/// by the lowest. None for no fills (the original's `min()` of nothing) or an invalid colour.
pub fn text_tone(fills: &[&str], dark: &str, light: &str) -> Option<(&'static str, f64)> {
    if fills.is_empty() {
        return None;
    }
    let lowest = |ink: &str| -> Option<f64> {
        let mut m: Option<f64> = None;
        for f in fills {
            let k = contrast(ink, f)?;
            m = Some(m.map_or(k, |m| py_min(m, k)));
        }
        m
    };
    let (kd, kl) = (lowest(dark)?, lowest(light)?);
    Some(if kd >= kl {
        ("dark", kd)
    } else {
        ("light", kl)
    })
}

// ---- OKHSL (okhsl.py) ------------------------------------------------------------------------

const W: [[f64; 3]; 3] = [
    [4.0767416621, -3.3077115913, 0.2309699292],
    [-1.2684380046, 2.6097574011, -0.3413193965],
    [-0.0041960863, -0.7034186147, 1.7076147010],
];

fn compute_max_saturation(a: f64, b: f64) -> f64 {
    let (k, w) = if -1.88170328 * a - 0.80936493 * b > 1. {
        (
            [1.19086277, 1.76576728, 0.59662641, 0.75515197, 0.56771245],
            W[0],
        )
    } else if 1.81444104 * a - 1.19445276 * b > 1. {
        (
            [0.73956515, -0.45954404, 0.08285427, 0.12541070, 0.14503204],
            W[1],
        )
    } else {
        (
            [
                1.35733652,
                -0.00915799,
                -1.15130210,
                -0.50559606,
                0.00692167,
            ],
            W[2],
        )
    };
    let s = k[0] + k[1] * a + k[2] * b + k[3] * a * a + k[4] * a * b;
    let k_l = 0.3963377774 * a + 0.2158037573 * b;
    let k_m = -0.1055613458 * a - 0.0638541728 * b;
    let k_s = -0.0894841775 * a - 1.2914855480 * b;
    let (l_, m_, s_) = (1. + s * k_l, 1. + s * k_m, 1. + s * k_s);
    let (l, m, ss) = (l_.powf(3.), m_.powf(3.), s_.powf(3.));
    let d1 = [3. * k_l * l_ * l_, 3. * k_m * m_ * m_, 3. * k_s * s_ * s_];
    let d2 = [
        6. * k_l * k_l * l_,
        6. * k_m * k_m * m_,
        6. * k_s * k_s * s_,
    ];
    let f = w[0] * l + w[1] * m + w[2] * ss;
    let f1 = w[0] * d1[0] + w[1] * d1[1] + w[2] * d1[2];
    let f2 = w[0] * d2[0] + w[1] * d2[1] + w[2] * d2[2];
    s - f * f1 / (f1 * f1 - 0.5 * f * f2)
}

fn find_cusp(a: f64, b: f64) -> (f64, f64) {
    let s = compute_max_saturation(a, b);
    let [r, g, bb] = oklab_to_linear_srgb(1., s * a, s * b);
    let l = (1. / py_max(py_max(r, g), bb)).powf(1. / 3.);
    (l, l * s)
}

fn find_gamut_intersection(a: f64, b: f64, l1: f64, c1: f64, l0: f64, cusp: (f64, f64)) -> f64 {
    let (lc, cc) = cusp;
    if (l1 - l0) * cc - (lc - l0) * c1 <= 0. {
        return cc * l0 / (c1 * lc + cc * (l0 - l1));
    }
    let t = cc * (l0 - 1.) / (c1 * (lc - 1.) + cc * (l0 - l1));
    let (dl, dc) = (l1 - l0, c1);
    let k_l = 0.3963377774 * a + 0.2158037573 * b;
    let k_m = -0.1055613458 * a - 0.0638541728 * b;
    let k_s = -0.0894841775 * a - 1.2914855480 * b;
    let (ldt, mdt, sdt) = (dl + dc * k_l, dl + dc * k_m, dl + dc * k_s);
    let (l, c) = (l0 * (1. - t) + t * l1, t * c1);
    let (l_, m_, s_) = (l + c * k_l, l + c * k_m, l + c * k_s);
    let (ll, m, s) = (l_.powf(3.), m_.powf(3.), s_.powf(3.));
    let d1 = [3. * ldt * l_ * l_, 3. * mdt * m_ * m_, 3. * sdt * s_ * s_];
    let d2 = [
        6. * ldt * ldt * l_,
        6. * mdt * mdt * m_,
        6. * sdt * sdt * s_,
    ];
    let mut best = f64::INFINITY;
    for w in W {
        let f = w[0] * ll + w[1] * m + w[2] * s - 1.;
        let f1 = w[0] * d1[0] + w[1] * d1[1] + w[2] * d1[2];
        let f2 = w[0] * d2[0] + w[1] * d2[1] + w[2] * d2[2];
        let u = f1 / (f1 * f1 - 0.5 * f * f2);
        if u >= 0. {
            best = py_min(best, -f * u);
        }
    }
    t + best
}

fn toe(x: f64) -> f64 {
    let (k1, k2) = (0.206, 0.03);
    let k3 = (1. + k1) / (1. + k2);
    0.5 * (k3 * x - k1 + ((k3 * x - k1).powf(2.) + 4. * k2 * k3 * x).sqrt())
}
fn toe_inv(x: f64) -> f64 {
    let (k1, k2) = (0.206, 0.03);
    let k3 = (1. + k1) / (1. + k2);
    (x * x + k1 * x) / (k3 * (x + k2))
}
fn st_mid(a: f64, b: f64) -> (f64, f64) {
    let s = 0.11516993
        + 1. / (7.44778970
            + 4.15901240 * b
            + a * (-2.19557347
                + 1.75198401 * b
                + a * (-2.13704948 - 10.02301043 * b
                    + a * (-4.24894561 + 5.38770819 * b + 4.69891013 * a))));
    let t = 0.11239642
        + 1. / (1.61320320 - 0.68124379 * b
            + a * (0.40370612
                + 0.90148123 * b
                + a * (-0.27087943
                    + 0.61223990 * b
                    + a * (0.00299215 - 0.45399568 * b - 0.14661872 * a))));
    (s, t)
}
fn get_cs(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let cusp = find_cusp(a, b);
    let c_max = find_gamut_intersection(a, b, l, 1., l, cusp);
    let (s_max, t_max) = (cusp.1 / cusp.0, cusp.1 / (1. - cusp.0));
    let k = c_max / py_min(l * s_max, (1. - l) * t_max);
    let (s_mid, t_mid) = st_mid(a, b);
    let (ca, cb) = (l * s_mid, (1. - l) * t_mid);
    let c_mid = 0.9 * k * (1. / (1. / ca.powf(4.) + 1. / cb.powf(4.))).sqrt().sqrt();
    let (ca, cb) = (l * 0.4, (1. - l) * 0.8);
    let c0 = (1. / (1. / ca.powf(2.) + 1. / cb.powf(2.))).sqrt();
    (c0, c_mid, c_max)
}

/// Gamma-encoded sRGB of an OKHSL colour (h degrees, s and l percent).
pub fn okhsl_to_srgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    let (s, l) = (s / 100., l / 100.);
    if l >= 1. {
        return [1.; 3];
    }
    if l <= 0. {
        return [0.; 3];
    }
    let (a, b) = (radians(h).cos(), radians(h).sin());
    let ll = toe_inv(l);
    let (c0, c_mid, c_max) = get_cs(ll, a, b);
    let (mid, mid_inv) = (0.8, 1.25);
    let c = if s < mid {
        let t = mid_inv * s;
        let k1 = mid * c0;
        let k2 = 1. - k1 / c_mid;
        t * k1 / (1. - k2 * t)
    } else {
        let t = (s - mid) / (1. - mid);
        let k0 = c_mid;
        let k1 = (1. - mid) * c_mid * c_mid * mid_inv * mid_inv / c0;
        let k2 = 1. - k1 / (c_max - c_mid);
        k0 + t * k1 / (1. - k2 * t)
    };
    oklab_to_linear_srgb(ll, c * a, c * b).map(to_gamma)
}

/// (h degrees, s percent, l percent) of gamma-encoded sRGB.
pub fn srgb_to_okhsl(r: f64, g: f64, b: f64) -> [f64; 3] {
    let [l, a, bb] = linear_srgb_to_oklab(from_gamma(r), from_gamma(g), from_gamma(b));
    let c = a.hypot(bb);
    if c < 1e-9 || l > 0.9999 {
        return [0., 0., 100. * toe(l)];
    }
    let (a_, b_) = (a / c, bb / c);
    let h = py_mod(degrees(bb.atan2(a)), 360.);
    let (c0, c_mid, c_max) = get_cs(l, a_, b_);
    let (mid, mid_inv) = (0.8, 1.25);
    let s = if c < c_mid {
        let k1 = mid * c0;
        let k2 = 1. - k1 / c_mid;
        mid * c / (k1 + k2 * c)
    } else {
        let k0 = c_mid;
        let k1 = (1. - mid) * c_mid * c_mid * mid_inv * mid_inv / c0;
        let k2 = 1. - k1 / (c_max - c_mid);
        mid + (1. - mid) * (c - k0) / (k1 + k2 * (c - k0))
    };
    [h, 100. * s, 100. * toe(l)]
}

/// The hex code of an OKHSL colour (okhsl.okhsl; its rounding is not clamped, as the original's).
pub fn okhsl(h: f64, s: f64, l: f64) -> String {
    okhsl_to_srgb(h, s, l)
        .iter()
        .fold(String::from("#"), |mut o, v| {
            o.push_str(&format!("{:02x}", round_even(255. * v) as i64));
            o
        })
}

/// OKLab (L, a, b) of a hex colour.
pub fn oklab(h: &str) -> Option<[f64; 3]> {
    let [l, c, hh] = oklch(h)?;
    Some([l, c * hh.to_radians().cos(), c * hh.to_radians().sin()])
}

/// Machado, Oliveira and Fernandes (2009), severity 1.0, in linear sRGB (svglint.CVD).
pub const CVD: [(&str, [[f64; 3]; 3]); 3] = [
    (
        "protanopia",
        [
            [0.152286, 1.052583, -0.204868],
            [0.114503, 0.786281, 0.099216],
            [-0.003882, -0.048116, 1.051998],
        ],
    ),
    (
        "deuteranopia",
        [
            [0.367322, 0.860646, -0.227968],
            [0.280085, 0.672501, 0.047413],
            [-0.011820, 0.042940, 0.968881],
        ],
    ),
    (
        "tritanopia",
        [
            [1.255528, -0.076749, -0.178779],
            [-0.078411, 0.930809, 0.147602],
            [0.004733, 0.691367, 0.303900],
        ],
    ),
];

/// The OKLab distance of two colours under a simulation matrix (svglint._cvd_distance).
pub fn cvd_distance(a: &str, b: &str, matrix: &[[f64; 3]; 3]) -> Option<f64> {
    let lab = |c: &str| -> Option<[f64; 3]> {
        let lin = hex_rgb(c)?.map(from_gamma);
        let sim = [0, 1, 2].map(|i| {
            (matrix[i][0] * lin[0] + matrix[i][1] * lin[1] + matrix[i][2] * lin[2]).clamp(0., 1.)
        });
        Some(linear_srgb_to_oklab(sim[0], sim[1], sim[2]))
    };
    let (x, y) = (lab(a)?, lab(b)?);
    Some(((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt())
}
