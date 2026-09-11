pub(crate) fn worker_count() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rayon::current_num_threads().max(1)
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

pub(crate) fn map_ordered<T, R, F>(items: Vec<T>, map: F) -> Vec<R>
where
    T: Send,
    R: Send,
    F: Fn(T) -> R + Send + Sync,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        items.into_par_iter().map(map).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        items.into_iter().map(map).collect()
    }
}

pub(crate) fn map_slice<T, R, F>(items: &[T], map: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Send + Sync,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        items.par_iter().map(map).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        items.iter().map(map).collect()
    }
}

pub(crate) fn filter_map_slice<T, R, F>(items: &[T], map: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> Option<R> + Send + Sync,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        items.par_iter().filter_map(map).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        items.iter().filter_map(map).collect()
    }
}

pub(crate) fn map_chunks<T, R, F>(items: &[T], chunk_size: usize, map: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&[T]) -> R + Send + Sync,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        items.par_chunks(chunk_size).map(map).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        items.chunks(chunk_size).map(map).collect()
    }
}

pub(crate) fn map_chunks_indexed<T, R, F>(items: &[T], chunk_size: usize, map: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &[T]) -> R + Send + Sync,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        items
            .par_chunks(chunk_size)
            .enumerate()
            .map(|(index, chunk)| map(index, chunk))
            .collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        items
            .chunks(chunk_size)
            .enumerate()
            .map(|(index, chunk)| map(index, chunk))
            .collect()
    }
}

pub(crate) fn map_mut<T, R, F>(items: &mut [T], map: F) -> Vec<R>
where
    T: Send,
    R: Send,
    F: Fn(&mut T) -> R + Send + Sync,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        items.par_iter_mut().map(map).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        items.iter_mut().map(map).collect()
    }
}

pub(crate) fn for_each_mut<T, F>(items: &mut [T], map: F)
where
    T: Send,
    F: Fn(&mut T) + Send + Sync,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        items.par_iter_mut().for_each(map);
    }
    #[cfg(target_arch = "wasm32")]
    {
        items.iter_mut().for_each(map);
    }
}
