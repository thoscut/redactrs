//! 2D-Transformationsmatrizen, wie PDF sie verwendet: `[a b c d e f]`.

use redact_core::Point;

/// Affine Transformation in PDF-Notation.
///
/// ```text
/// | a b 0 |
/// | c d 0 |
/// | e f 1 |
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Matrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    pub const fn translate(tx: f64, ty: f64) -> Self {
        Self::new(1.0, 0.0, 0.0, 1.0, tx, ty)
    }

    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self::new(sx, 0.0, 0.0, sy, 0.0, 0.0)
    }

    /// `self × other` — in PDF wird von links nach rechts angewendet:
    /// `cm` setzt `CTM_neu = M × CTM_alt`.
    pub fn mul(&self, other: &Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    pub fn apply(&self, x: f64, y: f64) -> Point {
        Point::new(
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    pub fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// Inverse Matrix, falls sie existiert.
    pub fn invert(&self) -> Option<Matrix> {
        let det = self.determinant();
        if det.abs() < 1e-12 {
            return None;
        }
        let inv_det = 1.0 / det;
        Some(Matrix {
            a: self.d * inv_det,
            b: -self.b * inv_det,
            c: -self.c * inv_det,
            d: self.a * inv_det,
            e: (self.c * self.f - self.d * self.e) * inv_det,
            f: (self.b * self.e - self.a * self.f) * inv_det,
        })
    }

    pub fn is_identity(&self) -> bool {
        (self.a - 1.0).abs() < 1e-9
            && self.b.abs() < 1e-9
            && self.c.abs() < 1e-9
            && (self.d - 1.0).abs() < 1e-9
            && self.e.abs() < 1e-9
            && self.f.abs() < 1e-9
    }

    /// Ungefährer Skalierungsfaktor (für Heuristiken wie Zeilenabstände).
    pub fn scale_hint(&self) -> f64 {
        let sx = (self.a * self.a + self.b * self.b).sqrt();
        let sy = (self.c * self.c + self.d * self.d).sqrt();
        ((sx + sy) / 2.0).max(1e-6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplication_order_matches_pdf() {
        // Erst um 10/20 verschieben, dann um Faktor 2 skalieren:
        // In PDF: `10 20 Td` gefolgt von einer Skalierungs-CTM.
        let t = Matrix::translate(10.0, 20.0);
        let s = Matrix::scale(2.0, 2.0);
        let m = t.mul(&s);
        assert_eq!(m.apply(0.0, 0.0), Point::new(20.0, 40.0));
    }

    #[test]
    fn inverse_roundtrip() {
        let m = Matrix::new(2.0, 0.5, -0.25, 3.0, 17.0, -4.0);
        let inv = m.invert().unwrap();
        let p = m.apply(7.0, 11.0);
        let back = inv.apply(p.x, p.y);
        assert!((back.x - 7.0).abs() < 1e-9);
        assert!((back.y - 11.0).abs() < 1e-9);
        assert!(m.mul(&inv).is_identity());
    }

    #[test]
    fn singular_matrix_has_no_inverse() {
        assert!(Matrix::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0).invert().is_none());
    }
}
