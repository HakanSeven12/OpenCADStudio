//! Lossless storage for the low component of double-single wire points.

#[derive(Clone, Debug)]
pub struct PointResiduals(Storage);

#[derive(Clone, Debug)]
enum Storage {
    Xyz(Vec<[f32; 3]>),
    Xy(Box<[[f32; 2]]>),
}

impl Default for PointResiduals {
    fn default() -> Self {
        Self(Storage::Xyz(Vec::new()))
    }
}

impl From<Vec<[f32; 3]>> for PointResiduals {
    fn from(values: Vec<[f32; 3]>) -> Self {
        Self(Storage::Xyz(values))
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ResidualView<'a> {
    Xyz(&'a [[f32; 3]]),
    Xy(&'a [[f32; 2]]),
}

impl ResidualView<'_> {
    #[inline]
    pub(crate) fn get(self, index: usize) -> Option<[f32; 3]> {
        match self {
            Self::Xyz(values) => values.get(index).copied(),
            Self::Xy(values) => values.get(index).map(|&[x, y]| [x, y, 0.0]),
        }
    }
}

impl PointResiduals {
    #[inline]
    pub(crate) fn view(&self) -> ResidualView<'_> {
        match &self.0 {
            Storage::Xyz(values) => ResidualView::Xyz(values),
            Storage::Xy(values) => ResidualView::Xy(values),
        }
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<[f32; 3]> {
        self.view().get(index)
    }

    pub fn len(&self) -> usize {
        match &self.0 {
            Storage::Xyz(values) => values.len(),
            Storage::Xy(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn allocated_bytes(&self) -> usize {
        match &self.0 {
            Storage::Xyz(values) => values.capacity() * std::mem::size_of::<[f32; 3]>(),
            Storage::Xy(values) => std::mem::size_of_val(values.as_ref()),
        }
    }

    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = [f32; 3]> + '_ {
        (0..self.len()).map(|i| self.get(i).expect("in range"))
    }

    pub(crate) fn clear(&mut self) {
        match &mut self.0 {
            Storage::Xyz(values) => values.clear(),
            Storage::Xy(_) => *self = Self::default(),
        }
    }

    pub(crate) fn dense_mut(&mut self) -> &mut Vec<[f32; 3]> {
        if let Storage::Xy(values) = &self.0 {
            self.0 = Storage::Xyz(values.iter().map(|&[x, y]| [x, y, 0.0]).collect());
        }
        let Storage::Xyz(values) = &mut self.0 else {
            unreachable!()
        };
        values
    }

    pub(crate) fn into_dense(self) -> Vec<[f32; 3]> {
        match self.0 {
            Storage::Xyz(values) => values,
            Storage::Xy(values) => values.iter().map(|&[x, y]| [x, y, 0.0]).collect(),
        }
    }

    pub(crate) fn compact_planar(&mut self) {
        if let Storage::Xyz(values) = &self.0 {
            // A negative zero or any nonzero residual must survive bit for bit.
            if !values.is_empty() && values.iter().all(|p| p[2].to_bits() == 0) {
                self.0 = Storage::Xy(values.iter().map(|&[x, y, _]| [x, y]).collect());
            }
        }
    }

    pub(crate) fn hash_into(&self, h: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        match &self.0 {
            Storage::Xyz(values) => {
                0u8.hash(h);
                bytemuck::cast_slice::<_, u8>(values).hash(h);
            }
            Storage::Xy(values) => {
                1u8.hash(h);
                bytemuck::cast_slice::<_, u8>(values).hash(h);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planar_storage_preserves_bits_and_expands_for_mutation() {
        let points = vec![[0.01234, -0.025, 0.0], [-0.0, f32::NAN, 0.0]];
        let mut packed = PointResiduals::from(points.clone());
        let dense_bytes = packed.allocated_bytes();
        packed.compact_planar();
        assert!(matches!(packed.0, Storage::Xy(_)));
        assert!(packed.allocated_bytes() < dense_bytes);
        for (i, point) in points.iter().enumerate() {
            assert_eq!(
                packed.get(i).unwrap().map(f32::to_bits),
                point.map(f32::to_bits)
            );
        }
        let mut edited = packed.clone();
        edited.dense_mut()[0][2] = 0.25;
        assert_eq!(packed.get(0).unwrap()[2], 0.0);
        assert_eq!(edited.get(0).unwrap()[2], 0.25);
    }

    #[test]
    fn signed_zero_and_nonzero_z_stay_in_full_storage() {
        for z in [-0.0_f32, f32::from_bits(1), 0.125] {
            let mut residuals = PointResiduals::from(vec![[1.0, 2.0, z]]);
            residuals.compact_planar();
            assert!(matches!(residuals.0, Storage::Xyz(_)));
            assert_eq!(residuals.get(0).unwrap()[2].to_bits(), z.to_bits());
        }
    }
}
