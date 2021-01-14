//! # RealFFT: Real-to-complex FFT and complex-to-real iFFT based on RustFFT
//!
//! This library is a wrapper for RustFFT that avoids the need of converting the data to complex before performing a FFT.
//! If the length is even, it also enables faster computations by using a complex FFT of half the length.
//! It then packs a 2N long real vector into an N long complex vector, which is transformed using a standard FFT.
//! It then post-processes the result to give only the first half of the complex spectrum, as an N+1 long complex vector.
//!
//! The iFFT goes through the same steps backwards, to transform an N+1 long complex spectrum to a 2N long real result.
//!
//! The speed increase compared to just converting the input to a 2N long complex vector
//! and using a 2N long FFT depends on the length f the input data.
//! The largest improvements are for long FFTs and for lengths over around 1000 elements there is an improvement of about a factor 2.
//! The difference shrinks for shorter lengths, and around 30 elements there is no longer any difference.  
//!
//! ## Why use real-to-complex fft?
//! ### Using a complex-to-complex fft
//! A simple way to get the fft of a rea values vector is to convert it to complex, and using a complex-to-complex fft.
//!
//! Let's assume `x` is a 6 element long real vector:
//! ```text
//! x = [x0r, x1r, x2r, x3r, x4r, x5r]
//! ```
//!
//! Converted to complex, using the notation `(xNr, xNi)` for the complex value `xN`, this becomes:
//! ```text
//! x_c = [(x0r, 0), (x1r, 0), (x2r, 0), (x3r, 0), (x4r, 0, (x5r, 0)]
//! ```
//!
//!
//! The general result of `X = FFT(x)` is:
//! ```text
//! X = [(X0r, X0i), (X1r, X1i), (X2r, X2i), (X3r, X3i), (X4r, X4i), (X5r, X5i)]
//! ```
//!
//! However, because our `x` was real-valued, some of this is redundant:
//! ```text
//! FFT(x) = [(X0r, 0), (X1r, X1i), (X2r, X2i), (X3r, 0), (X2r, -X2i), (X1r, -X1i)]
//! ```
//!
//! As we can see, the output contains a fair bit of redundant data. But it still takes time for the FFT to calculate these values. Converting the input data to complex also takes a little bit of time.
//!
//! ### real-to-complex
//! Using a real-to-complex fft removes the need for converting the input data to complex.
//! It also avoids caclulating the redundant output values.
//!
//! The result is:
//! ```text
//! RealFFT(x) = [(X0r, 0), (X1r, X1i), (X2r, X2i), (X3r, 0)]
//! ```
//!
//! If the length instead had been 7, result would have been:
//! ```text
//! FFT(x) = [(X0r, 0), (X1r, X1i), (X2r, X2i), (X3r, X3i), (X3r, -X3i), (X2r, -X2i), (X1r, -X1i)]
//! ```
//! After removing the reduntant elements, the result is:
//! ```text
//! RealFFT(x) = [(X0r, 0), (X1r, X1i), (X2r, X2i), (X3r, X3i)]
//! ```
//!
//! This is the data layout output by the real-to-complex fft, and the one expected as input to the complex-to-real ifft.
//!
//! ## Scaling
//! RealFFT matches the behaviour of RustFFT and does not normalize the output of either FFT of iFFT. To get normalized results, each element must be scaled by `1/sqrt(length)`. If the processing involves both an FFT and an iFFT step, it is advisable to merge the two normalization steps to a single, by scaling by `1/length`.
//!
//! ## Documentation
//!
//! The full documentation can be generated by rustdoc. To generate and view it run:
//! ```text
//! cargo doc --open
//! ```
//!
//! ## Benchmarks
//!
//! To run a set of benchmarks comparing real-to-complex FFT with standard complex-to-complex, type:
//! ```text
//! cargo bench
//! ```
//! The results are printed while running, and are compiled into an html report containing much more details.
//! To view, open `target/criterion/report/index.html` in a browser.
//!
//! ## Example
//! Transform a vector, and then inverse transform the result.
//! ```
//! use realfft::RealFftPlanner;
//! use rustfft::num_complex::Complex;
//! use rustfft::num_traits::Zero;
//!
//! // make dummy input vector, spectrum and output vectors
//! let mut indata = vec![0.0f64; 256];
//! let mut spectrum: Vec<Complex<f64>> = vec![Complex::zero(); 129];
//! let mut outdata: Vec<f64> = vec![0.0; 256];
//!
//! // make a planner
//! let mut real_planner = RealFftPlanner::<f64>::new();
//!
//! //create an FFT and forward transform the input data
//! let r2c = real_planner.plan_fft_forward(256);
//! r2c.process(&mut indata, &mut spectrum).unwrap();
//!
//! // create an iFFT and inverse transform the spectum
//! let c2r = real_planner.plan_fft_inverse(256);
//! c2r.process(&mut spectrum, &mut outdata).unwrap();
//! ```
//!
//! ## Compatibility
//!
//! The `realfft` crate requires rustc version 1.37 or newer.

use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use rustfft::{FftNum, FftPlanner};
use std::collections::HashMap;
use std::error;
use std::fmt;
use std::sync::Arc;

type Res<T> = Result<T, Box<dyn error::Error>>;

/// Custom error returned by FFTs
#[derive(Debug)]
pub struct FftError {
    desc: String,
}

impl fmt::Display for FftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.desc)
    }
}

impl error::Error for FftError {
    fn description(&self) -> &str {
        &self.desc
    }
}

impl FftError {
    pub fn new(desc: &str) -> Self {
        FftError {
            desc: desc.to_owned(),
        }
    }
}

fn compute_twiddle<T: FftNum>(index: usize, fft_len: usize) -> Complex<T> {
    let constant = -2f64 * std::f64::consts::PI / fft_len as f64;
    let angle = constant * index as f64;
    Complex {
        re: T::from_f64(angle.cos()).unwrap(),
        im: T::from_f64(angle.sin()).unwrap(),
    }
}

struct RealToComplexOdd<T> {
    length: usize,
    fft: std::sync::Arc<dyn rustfft::Fft<T>>,
    scratch_len: usize,
}

struct RealToComplexEven<T> {
    twiddles: Vec<Complex<T>>,
    length: usize,
    fft: std::sync::Arc<dyn rustfft::Fft<T>>,
    scratch_len: usize,
}

struct ComplexToRealOdd<T> {
    length: usize,
    fft: std::sync::Arc<dyn rustfft::Fft<T>>,
    scratch_len: usize,
}

struct ComplexToRealEven<T> {
    twiddles: Vec<Complex<T>>,
    length: usize,
    fft: std::sync::Arc<dyn rustfft::Fft<T>>,
    scratch_len: usize,
}

/// An FFT that takes a real-valued input vector of length 2*N and transforms it to a complex
/// spectrum of length N+1.
pub trait RealToComplex<T> {
    /// Transform a vector of 2*N real-valued samples, storing the result in the N+1 element long complex output vector.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also allocates additional scratch space as needed.  
    fn process(&self, input: &mut [T], output: &mut [Complex<T>]) -> Res<()>;

    /// Transform a vector of 2*N real-valued samples, storing the result in the N+1 element long complex output vector.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also uses the provided scratch vector instead of allocating, which will be faster if it is called more than once.
    fn process_with_scratch(
        &self,
        input: &mut [T],
        output: &mut [Complex<T>],
        scratch: &mut [Complex<T>],
    ) -> Res<()>;

    /// Get the length of the scratch space needed for `process_with_scratch`.
    fn get_scratch_len(&self) -> usize;
}

/// An FFT that takes a complex-valued input vector of length N+1 and transforms it to a complex
/// spectrum of length 2*N.
pub trait ComplexToReal<T> {
    /// Transform a complex spectrum of N+1 values and store the real result in the 2*N long output.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also allocates additional scratch space as needed.
    fn process(&self, input: &mut [Complex<T>], output: &mut [T]) -> Res<()>;

    /// Transform a complex spectrum of N+1 values and store the real result in the 2*N long output.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also uses the provided scratch vector instead of allocating, which will be faster if it is called more than once.
    fn process_with_scratch(
        &self,
        input: &mut [Complex<T>],
        output: &mut [T],
        scratch: &mut [Complex<T>],
    ) -> Res<()>;

    /// Get the length of the scratch space needed for `process_with_scratch`.
    fn get_scratch_len(&self) -> usize;
}

pub fn zip3<A, B, C>(a: A, b: B, c: C) -> impl Iterator<Item = (A::Item, B::Item, C::Item)>
where
    A: IntoIterator,
    B: IntoIterator,
    C: IntoIterator,
{
    a.into_iter()
        .zip(b.into_iter().zip(c))
        .map(|(x, (y, z))| (x, y, z))
}

/// A planner is used to create FFTs. It caches results internally,
/// so when making more than one FFT it is advisable to reuse the same planner.
pub struct RealFftPlanner<T: FftNum> {
    planner: FftPlanner<T>,
    r2c_cache: HashMap<usize, Arc<dyn RealToComplex<T>>>,
    c2r_cache: HashMap<usize, Arc<dyn ComplexToReal<T>>>,
}

impl<T: FftNum> RealFftPlanner<T> {
    /// Create a new planner.
    pub fn new() -> Self {
        let planner = FftPlanner::<T>::new();
        Self {
            r2c_cache: HashMap::new(),
            c2r_cache: HashMap::new(),
            planner,
        }
    }

    /// Plan a Real-to-Complex forward FFT.
    pub fn plan_fft_forward(&mut self, len: usize) -> Arc<dyn RealToComplex<T>> {
        if self.r2c_cache.contains_key(&len) {
            Arc::clone(self.r2c_cache.get(&len).unwrap())
        } else {
            let fft = if len % 2 > 0 {
                Arc::new(RealToComplexOdd::new(len, &mut self.planner)) as Arc<dyn RealToComplex<T>>
            } else {
                Arc::new(RealToComplexEven::new(len, &mut self.planner))
                    as Arc<dyn RealToComplex<T>>
            };
            self.r2c_cache.insert(len, Arc::clone(&fft));
            fft
        }
    }

    /// Plan a Complex-to-Real inverse FFT.
    pub fn plan_fft_inverse(&mut self, len: usize) -> Arc<dyn ComplexToReal<T>> {
        if self.c2r_cache.contains_key(&len) {
            Arc::clone(self.c2r_cache.get(&len).unwrap())
        } else {
            let fft = if len % 2 > 0 {
                Arc::new(ComplexToRealOdd::new(len, &mut self.planner)) as Arc<dyn ComplexToReal<T>>
            } else {
                Arc::new(ComplexToRealEven::new(len, &mut self.planner))
                    as Arc<dyn ComplexToReal<T>>
            };
            self.c2r_cache.insert(len, Arc::clone(&fft));
            fft
        }
    }
}

impl<T: FftNum> Default for RealFftPlanner<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: FftNum> RealToComplexOdd<T> {
    /// Create a new RealToComplex FFT for input data of a given length, and uses the given FftPlanner to build the inner FFT.
    /// Panics if the length is not odd.
    pub fn new(length: usize, fft_planner: &mut FftPlanner<T>) -> Self {
        if length % 2 == 0 {
            panic!("Length must be odd, got {}", length,);
        }
        let fft = fft_planner.plan_fft_forward(length);
        let scratch_len = fft.get_inplace_scratch_len() + length;
        RealToComplexOdd {
            length,
            fft,
            scratch_len,
        }
    }
}

impl<T: FftNum> RealToComplex<T> for RealToComplexOdd<T> {
    /// Transform a vector of 2*N real-valued samples, storing the result in the N+1 element long complex output vector.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also allocates additional scratch space as needed.  
    fn process(&self, input: &mut [T], output: &mut [Complex<T>]) -> Res<()> {
        let mut scratch = vec![Complex::zero(); self.scratch_len];
        self.process_with_scratch(input, output, &mut scratch)
    }

    /// Transform a vector of 2*N real-valued samples, storing the result in the N+1 element long complex output vector.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also uses the provided scratch vector instead of allocating, which will be faster if it is called more than once.
    fn process_with_scratch(
        &self,
        input: &mut [T],
        output: &mut [Complex<T>],
        scratch: &mut [Complex<T>],
    ) -> Res<()> {
        if input.len() != self.length {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of input, expected {}, got {}",
                    self.length,
                    input.len()
                )
                .as_str(),
            )));
        }
        if output.len() != (self.length / 2 + 1) {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of output, expected {}, got {}",
                    self.length / 2 + 1,
                    input.len()
                )
                .as_str(),
            )));
        }
        if scratch.len() != (self.scratch_len) {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of scratch, expected {}, got {}",
                    self.scratch_len / 2 + 1,
                    scratch.len()
                )
                .as_str(),
            )));
        }
        let (buffer, fft_scratch) = scratch.split_at_mut(self.length);

        for (val, buf) in input.iter().zip(buffer.iter_mut()) {
            *buf = Complex::new(*val, T::zero());
        }
        // FFT and store result in buffer_out
        #[cfg(not(feature = "dummyfft"))]
        self.fft.process_with_scratch(buffer, fft_scratch);
        output.copy_from_slice(&buffer[0..self.length / 2 + 1]);
        Ok(())
    }

    fn get_scratch_len(&self) -> usize {
        self.scratch_len
    }
}

impl<T: FftNum> RealToComplexEven<T> {
    /// Create a new RealToComplex FFT for input data of a given length, and uses the given FftPlanner to build the inner FFT.
    /// Panics if the length is not even.
    pub fn new(length: usize, fft_planner: &mut FftPlanner<T>) -> Self {
        if length % 2 > 0 {
            panic!("Length must be even, got {}", length,);
        }
        let twiddle_count = if length % 4 == 0 {
            length / 4
        } else {
            length / 4 + 1
        };
        let twiddles: Vec<Complex<T>> = (1..twiddle_count)
            .map(|i| compute_twiddle(i, length) * T::from_f64(0.5).unwrap())
            .collect();
        //let mut fft_planner = FftPlanner::<T>::new();
        let fft = fft_planner.plan_fft_forward(length / 2);
        let scratch_len = fft.get_outofplace_scratch_len();
        RealToComplexEven {
            twiddles,
            length,
            fft,
            scratch_len,
        }
    }
}

impl<T: FftNum> RealToComplex<T> for RealToComplexEven<T> {
    /// Transform a vector of 2*N real-valued samples, storing the result in the N+1 element long complex output vector.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also allocates additional scratch space as needed.  
    fn process(&self, input: &mut [T], output: &mut [Complex<T>]) -> Res<()> {
        let mut scratch = vec![Complex::zero(); self.scratch_len];
        self.process_with_scratch(input, output, &mut scratch)
    }

    /// Transform a vector of 2*N real-valued samples, storing the result in the N+1 element long complex output vector.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also uses the provided scratch vector instead of allocating, which will be faster if it is called more than once.
    fn process_with_scratch(
        &self,
        input: &mut [T],
        output: &mut [Complex<T>],
        scratch: &mut [Complex<T>],
    ) -> Res<()> {
        if input.len() != self.length {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of input, expected {}, got {}",
                    self.length,
                    input.len()
                )
                .as_str(),
            )));
        }
        if output.len() != (self.length / 2 + 1) {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of output, expected {}, got {}",
                    self.length / 2 + 1,
                    input.len()
                )
                .as_str(),
            )));
        }
        if scratch.len() != (self.scratch_len) {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of scratch, expected {}, got {}",
                    self.scratch_len / 2 + 1,
                    scratch.len()
                )
                .as_str(),
            )));
        }

        let fftlen = self.length / 2;
        let mut buf_in = unsafe {
            let ptr = input.as_mut_ptr() as *mut Complex<T>;
            let len = input.len();
            std::slice::from_raw_parts_mut(ptr, len / 2)
        };

        // FFT and store result in buffer_out
        #[cfg(not(feature = "dummyfft"))]
        self.fft
            .process_outofplace_with_scratch(&mut buf_in, &mut output[0..fftlen], scratch);
        let (mut output_left, mut output_right) = output.split_at_mut(output.len() / 2);

        // The first and last element don't require any twiddle factors, so skip that work
        match (output_left.first_mut(), output_right.last_mut()) {
            (Some(first_element), Some(last_element)) => {
                // The first and last elements are just a sum and difference of the first value's real and imaginary values
                let first_value = *first_element;
                *first_element = Complex {
                    re: first_value.re + first_value.im,
                    im: T::zero(),
                };
                *last_element = Complex {
                    re: first_value.re - first_value.im,
                    im: T::zero(),
                };

                // Chop the first and last element off of our slices so that the loop below doesn't have to deal with them
                output_left = &mut output_left[1..];
                let right_len = output_right.len();
                output_right = &mut output_right[..right_len - 1];
            }
            _ => {
                return Ok(());
            }
        }
        // Loop over the remaining elements and apply twiddle factors on them
        for (twiddle, out, out_rev) in zip3(
            self.twiddles.iter(),
            output_left.iter_mut(),
            output_right.iter_mut().rev(),
        ) {
            let sum = *out + *out_rev;
            let diff = *out - *out_rev;
            let half = T::from_f64(0.5).unwrap();
            // Apply twiddle factors. Theoretically we'd have to load 2 separate twiddle factors here, one for the beginning
            // and one for the end. But the twiddle factor for the end is jsut the twiddle for the beginning, with the
            // real part negated. Since it's the same twiddle, we can factor out a ton of math ops and cut the number of
            // multiplications in half
            let twiddled_re_sum = sum * twiddle.re;
            let twiddled_im_sum = sum * twiddle.im;
            let twiddled_re_diff = diff * twiddle.re;
            let twiddled_im_diff = diff * twiddle.im;
            let half_sum_re = half * sum.re;
            let half_diff_im = half * diff.im;

            let output_twiddled_real = twiddled_re_sum.im + twiddled_im_diff.re;
            let output_twiddled_im = twiddled_im_sum.im - twiddled_re_diff.re;

            // We finally have all the data we need to write the transformed data back out where we found it
            *out = Complex {
                re: half_sum_re + output_twiddled_real,
                im: half_diff_im + output_twiddled_im,
            };

            *out_rev = Complex {
                re: half_sum_re - output_twiddled_real,
                im: output_twiddled_im - half_diff_im,
            };
        }

        // If the output len is odd, the loop above can't postprocess the centermost element, so handle that separately
        if output.len() % 2 == 1 {
            if let Some(center_element) = output.get_mut(output.len() / 2) {
                center_element.im = -center_element.im;
            }
        }
        Ok(())
    }
    fn get_scratch_len(&self) -> usize {
        self.scratch_len
    }
}

impl<T: FftNum> ComplexToRealOdd<T> {
    /// Create a new ComplexToReal FFT for input data of a given length, and uses the given FftPlanner to build the inner FFT.
    /// Panics if the length is not odd.
    pub fn new(length: usize, fft_planner: &mut FftPlanner<T>) -> Self {
        if length % 2 == 0 {
            panic!("Length must be odd, got {}", length,);
        }
        //let mut fft_planner = FftPlanner::<T>::new();
        let fft = fft_planner.plan_fft_inverse(length);
        let scratch_len = length + fft.get_inplace_scratch_len();
        ComplexToRealOdd {
            length,
            fft,
            scratch_len,
        }
    }
}

impl<T: FftNum> ComplexToReal<T> for ComplexToRealOdd<T> {
    /// Transform a complex spectrum of N+1 values and store the real result in the 2*N long output.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also allocates additional scratch space as needed.
    fn process(&self, input: &mut [Complex<T>], output: &mut [T]) -> Res<()> {
        let mut scratch = vec![Complex::zero(); self.scratch_len];
        self.process_with_scratch(input, output, &mut scratch)
    }

    /// Transform a complex spectrum of N+1 values and store the real result in the 2*N long output.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also uses the provided scratch vector instead of allocating, which will be faster if it is called more than once.
    fn process_with_scratch(
        &self,
        input: &mut [Complex<T>],
        output: &mut [T],
        scratch: &mut [Complex<T>],
    ) -> Res<()> {
        if input.len() != (self.length / 2 + 1) {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of input, expected {}, got {}",
                    self.length / 2 + 1,
                    input.len()
                )
                .as_str(),
            )));
        }
        if output.len() != self.length {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of output, expected {}, got {}",
                    self.length,
                    input.len()
                )
                .as_str(),
            )));
        }
        if scratch.len() != (self.scratch_len) {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of scratch, expected {}, got {}",
                    self.scratch_len / 2 + 1,
                    scratch.len()
                )
                .as_str(),
            )));
        }

        let (buffer, fft_scratch) = scratch.split_at_mut(self.length);

        buffer[0..input.len()].copy_from_slice(&input);
        for (buf, val) in buffer
            .iter_mut()
            .rev()
            .take(self.length / 2)
            .zip(input.iter().skip(1))
        {
            *buf = val.conj();
            //buf.im = -val.im;
        }
        #[cfg(not(feature = "dummyfft"))]
        self.fft.process_with_scratch(buffer, fft_scratch);
        for (val, out) in buffer.iter().zip(output.iter_mut()) {
            *out = val.re;
        }
        Ok(())
    }

    fn get_scratch_len(&self) -> usize {
        self.scratch_len
    }
}

impl<T: FftNum> ComplexToRealEven<T> {
    /// Create a new ComplexToReal FFT for input data of a given length, and uses the given FftPlanner to build the inner FFT.
    /// Panics if the length is not even.
    pub fn new(length: usize, fft_planner: &mut FftPlanner<T>) -> Self {
        if length % 2 > 0 {
            panic!("Length must be even, got {}", length,);
        }
        let twiddle_count = if length % 4 == 0 {
            length / 4
        } else {
            length / 4 + 1
        };
        let twiddles: Vec<Complex<T>> = (1..twiddle_count)
            .map(|i| compute_twiddle(i, length).conj())
            .collect();
        //let mut fft_planner = FftPlanner::<T>::new();
        let fft = fft_planner.plan_fft_inverse(length / 2);
        let scratch_len = fft.get_outofplace_scratch_len();
        ComplexToRealEven {
            twiddles,
            length,
            fft,
            scratch_len,
        }
    }
}
impl<T: FftNum> ComplexToReal<T> for ComplexToRealEven<T> {
    /// Transform a complex spectrum of N+1 values and store the real result in the 2*N long output.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also allocates additional scratch space as needed.
    fn process(&self, input: &mut [Complex<T>], output: &mut [T]) -> Res<()> {
        let mut scratch = vec![Complex::zero(); self.scratch_len];
        self.process_with_scratch(input, output, &mut scratch)
    }

    /// Transform a complex spectrum of N+1 values and store the real result in the 2*N long output.
    /// The input buffer is used as scratch space, so the contents of input should be considered garbage after calling.
    /// It also uses the provided scratch vector instead of allocating, which will be faster if it is called more than once.
    fn process_with_scratch(
        &self,
        input: &mut [Complex<T>],
        output: &mut [T],
        scratch: &mut [Complex<T>],
    ) -> Res<()> {
        if input.len() != (self.length / 2 + 1) {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of input, expected {}, got {}",
                    self.length / 2 + 1,
                    input.len()
                )
                .as_str(),
            )));
        }
        if output.len() != self.length {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of output, expected {}, got {}",
                    self.length,
                    input.len()
                )
                .as_str(),
            )));
        }
        if scratch.len() != (self.scratch_len) {
            return Err(Box::new(FftError::new(
                format!(
                    "Wrong length of scratch, expected {}, got {}",
                    self.scratch_len / 2 + 1,
                    scratch.len()
                )
                .as_str(),
            )));
        }
        let (mut input_left, mut input_right) = input.split_at_mut(input.len() / 2);

        // We have to preprocess the input in-place before we send it to the FFT.
        // The first and centermost values have to be preprocessed separately from the rest, so do that now
        match (input_left.first_mut(), input_right.last_mut()) {
            (Some(first_input), Some(last_input)) => {
                let first_sum = *first_input + *last_input;
                let first_diff = *first_input - *last_input;

                *first_input = Complex {
                    re: first_sum.re - first_sum.im,
                    im: first_diff.re - first_diff.im,
                };

                input_left = &mut input_left[1..];
                let right_len = input_right.len();
                input_right = &mut input_right[..right_len - 1];
            }
            _ => return Ok(()),
        };

        // now, in a loop, preprocess the rest of the elements 2 at a time
        for (twiddle, fft_input, fft_input_rev) in zip3(
            self.twiddles.iter(),
            input_left.iter_mut(),
            input_right.iter_mut().rev(),
        ) {
            let sum = *fft_input + *fft_input_rev;
            let diff = *fft_input - *fft_input_rev;

            // Apply twiddle factors. Theoretically we'd have to load 2 separate twiddle factors here, one for the beginning
            // and one for the end. But the twiddle factor for the end is jsut the twiddle for the beginning, with the
            // real part negated. Since it's the same twiddle, we can factor out a ton of math ops and cut the number of
            // multiplications in half
            let twiddled_re_sum = sum * twiddle.re;
            let twiddled_im_sum = sum * twiddle.im;
            let twiddled_re_diff = diff * twiddle.re;
            let twiddled_im_diff = diff * twiddle.im;

            let output_twiddled_real = twiddled_re_sum.im + twiddled_im_diff.re;
            let output_twiddled_im = twiddled_im_sum.im - twiddled_re_diff.re;

            // We finally have all the data we need to write our preprocessed data back where we got it from
            *fft_input = Complex {
                re: sum.re - output_twiddled_real,
                im: diff.im - output_twiddled_im,
            };
            *fft_input_rev = Complex {
                re: sum.re + output_twiddled_real,
                im: -output_twiddled_im - diff.im,
            }
        }

        // If the output len is odd, the loop above can't preprocess the centermost element, so handle that separately
        if input.len() % 2 == 1 {
            let center_element = input[input.len() / 2];
            let doubled = center_element + center_element;
            input[input.len() / 2] = doubled.conj();
        }

        // FFT and store result in buffer_out
        let mut buf_out = unsafe {
            let ptr = output.as_mut_ptr() as *mut Complex<T>;
            let len = output.len();
            std::slice::from_raw_parts_mut(ptr, len / 2)
        };
        #[cfg(not(feature = "dummyfft"))]
        self.fft.process_outofplace_with_scratch(
            &mut input[..output.len() / 2],
            &mut buf_out,
            scratch,
        );
        Ok(())
    }

    fn get_scratch_len(&self) -> usize {
        self.scratch_len
    }
}

#[cfg(test)]
mod tests {
    use crate::RealFftPlanner;
    use rustfft::num_complex::Complex;
    use rustfft::num_traits::Zero;
    use rustfft::FftPlanner;

    fn compare_complex(a: &[Complex<f64>], b: &[Complex<f64>], tol: f64) -> bool {
        a.iter().zip(b.iter()).fold(true, |eq, (val_a, val_b)| {
            eq && (val_a.re - val_b.re).abs() < tol && (val_a.im - val_b.im).abs() < tol
        })
    }

    fn compare_f64(a: &[f64], b: &[f64], tol: f64) -> bool {
        a.iter()
            .zip(b.iter())
            .fold(true, |eq, (val_a, val_b)| eq && (val_a - val_b).abs() < tol)
    }

    // Compare ComplexToReal with standard iFFT
    #[test]
    fn complex_to_real() {
        for length in 5..7 {
            let mut indata: Vec<Complex<f64>> = vec![Complex::zero(); length / 2 + 1];
            let mut rustfft_check: Vec<Complex<f64>> = vec![Complex::zero(); length];
            for (n, val) in indata.iter_mut().enumerate() {
                *val = Complex::new(n as f64, (2 * n) as f64);
            }
            indata[0].im = 0.0;
            if length % 2 == 0 {
                indata[length / 2].im = 0.0;
            }
            for (val_long, val) in rustfft_check
                .iter_mut()
                .take(length / 2 + 1)
                .zip(indata.iter())
            {
                *val_long = *val;
            }
            for (val_long, val) in rustfft_check
                .iter_mut()
                .rev()
                .take(length / 2)
                .zip(indata.iter().skip(1))
            {
                *val_long = val.conj();
            }
            let mut fft_planner = FftPlanner::<f64>::new();
            let fft = fft_planner.plan_fft_inverse(length);

            let mut real_planner = RealFftPlanner::<f64>::new();
            let c2r = real_planner.plan_fft_inverse(length);
            let mut out_a: Vec<f64> = vec![0.0; length];
            c2r.process(&mut indata, &mut out_a).unwrap();
            fft.process(&mut rustfft_check);

            let check_real = rustfft_check.iter().map(|val| val.re).collect::<Vec<f64>>();
            assert!(compare_f64(&out_a, &check_real, 1.0e-9));
        }
    }

    // Compare RealToComplex with standard FFT
    #[test]
    fn real_to_complex() {
        for length in 2..64 {
            let mut indata = vec![0.0f64; length];
            for (n, val) in indata.iter_mut().enumerate() {
                *val = n as f64;
            }
            let mut rustfft_check = indata
                .iter()
                .map(|val| Complex::from(val))
                .collect::<Vec<Complex<f64>>>();
            let mut fft_planner = FftPlanner::<f64>::new();
            let fft = fft_planner.plan_fft_forward(length);

            let mut real_planner = RealFftPlanner::<f64>::new();
            let r2c = real_planner.plan_fft_forward(length);
            let mut out_a: Vec<Complex<f64>> = vec![Complex::zero(); length / 2 + 1];

            fft.process(&mut rustfft_check);
            r2c.process(&mut indata, &mut out_a).unwrap();
            assert!(compare_complex(
                &out_a,
                &rustfft_check[0..(length / 2 + 1)],
                1.0e-9
            ));
        }
    }
}
