#![allow(unsafe_code)]

use std::any::Any;
use std::cell::RefCell;

use bytemuck::cast_vec;
use fast_image_resize::{IntoImageView, PixelType, ResizeOptions, Resizer, images::Image};
use ffmpeg::{
    frame::Video,
    software::scaling::{context::Context as ScalingContext, flag::Flags},
    util::format::pixel::Pixel as PixelFormat,
};
use image::{ColorType, DynamicImage, GenericImageView, GrayImage, ImageBuffer};

thread_local! {
    static THREAD_RESIZER: RefCell<Resizer> = RefCell::new({
        #[allow(unused_mut)]
        let mut resizer = Resizer::new();
        #[cfg(target_arch = "aarch64")]
        #[allow(unsafe_code)]
        unsafe {
            if !crate::simd::neon_enabled() {
                resizer.set_cpu_extensions(fast_image_resize::CpuExtensions::None);
            }
        }
        resizer
    });
}

pub fn resize_to_minium_canonical_image(image: &DynamicImage, tc: f32, dot_tau_ti: f32) -> DynamicImage {
    let (image_width, image_height) = image.dimensions();

    let scale_factor = tc / dot_tau_ti;

    let resized_width = (scale_factor * image_width as f32) as u32;
    let resized_height = (scale_factor * image_height as f32) as u32;

    resize_fast(image, resized_width, resized_height)
}

pub fn resize_fast(image: &(impl IntoImageView + Any), width: u32, height: u32) -> DynamicImage {
    THREAD_RESIZER.with(|r| {
        let mut resizer = r.borrow_mut();
        resize_fast_with_resizer(&mut resizer, image, width, height)
    })
}

pub fn resize_fast_with_resizer(resizer: &mut Resizer, image: &(impl IntoImageView + Any), width: u32, height: u32) -> DynamicImage {
    let width = width.max(1);
    let height = height.max(1);

    if image.width() == width
        && image.height() == height
        && let Some(img) = (image as &dyn Any).downcast_ref::<DynamicImage>()
    {
        return img.clone();
    }
    // Create a destination image with the new dimensions
    let pixel_type = image.pixel_type().unwrap();
    let mut dst_image = Image::new(width, height, pixel_type);
    let has_alpha = matches!(pixel_type, PixelType::U8x2 | PixelType::U8x4 | PixelType::U16x2 | PixelType::U16x4 | PixelType::F32x4);
    let options = ResizeOptions::new().resize_alg(fast_image_resize::ResizeAlg::Nearest).use_alpha(has_alpha);
    // Perform the resize operation
    resizer.resize(image, &mut dst_image, &options).expect("Failed to resize image");

    // Convert the resized image back to a DynamicImage
    let buffer: Vec<u8> = dst_image.into_vec();
    match pixel_type {
        PixelType::U8 => {
            let buf = ImageBuffer::from_vec(width, height, buffer).expect("Failed to create Luma8 image");
            DynamicImage::ImageLuma8(buf)
        }
        PixelType::U8x2 => {
            let buf = ImageBuffer::from_vec(width, height, buffer).expect("Failed to create LumaA8 image");
            DynamicImage::ImageLumaA8(buf)
        }
        PixelType::U8x3 => {
            let buf = ImageBuffer::from_vec(width, height, buffer).expect("Failed to create RgbImage");
            DynamicImage::ImageRgb8(buf)
        }
        PixelType::U8x4 => {
            let buf = ImageBuffer::from_vec(width, height, buffer).expect("Failed to create RgbaImage");
            DynamicImage::ImageRgba8(buf)
        }
        PixelType::U16 => {
            let buf = ImageBuffer::from_vec(width, height, cast_vec(buffer)).expect("Failed to create Luma16 image");
            DynamicImage::ImageLuma16(buf)
        }
        PixelType::U16x2 => {
            let buf = ImageBuffer::from_vec(width, height, cast_vec(buffer)).expect("Failed to create LumaA16 image");
            DynamicImage::ImageLumaA16(buf)
        }
        PixelType::U16x3 => {
            let buf = ImageBuffer::from_vec(width, height, cast_vec(buffer)).expect("Failed to create Rgb16 image");
            DynamicImage::ImageRgb16(buf)
        }
        PixelType::U16x4 => {
            let buf = ImageBuffer::from_vec(width, height, cast_vec(buffer)).expect("Failed to create Rgba16 image");
            DynamicImage::ImageRgba16(buf)
        }
        PixelType::F32x3 => {
            let buf = ImageBuffer::from_vec(width, height, cast_vec(buffer)).expect("Failed to create Rgb32F image");
            DynamicImage::ImageRgb32F(buf)
        }
        PixelType::F32x4 => {
            let buf = ImageBuffer::from_vec(width, height, cast_vec(buffer)).expect("Failed to create Rgba32F image");
            DynamicImage::ImageRgba32F(buf)
        }
        other => panic!("Unsupported pixel type {other:?}"),
    }
}

pub fn downscale_luma8_in_place(gray: GrayImage, dst_width: u32, dst_height: u32) -> GrayImage {
    let dst_width = dst_width.max(1);
    let dst_height = dst_height.max(1);
    let (src_width, src_height) = gray.dimensions();
    if src_width == dst_width && src_height == dst_height {
        return gray;
    }

    let mut buf = gray.into_raw();
    let x_scale = src_width as f64 / dst_width as f64;
    let y_scale = src_height as f64 / dst_height as f64;
    let x_in_start = x_scale * 0.5;
    let max_src_x = src_width as usize;
    let mut x_in_tab = Vec::with_capacity(dst_width as usize);
    for x in 0..dst_width {
        let x_in = ((x_in_start + x_scale * x as f64) as usize).min(max_src_x);
        x_in_tab.push(x_in);
    }

    let y_in_start = y_scale * 0.5;
    let src_stride = src_width as usize;
    let mut dst_idx = 0usize;
    for y in 0..dst_height {
        let y_in = (y_in_start + y_scale * y as f64) as usize;
        let src_row = y_in * src_stride;
        for &x_in in &x_in_tab {
            buf[dst_idx] = buf[src_row + x_in];
            dst_idx += 1;
        }
    }
    buf.truncate(dst_idx);
    ImageBuffer::from_vec(dst_width, dst_height, buf).expect("Failed to create Luma8 image")
}

pub fn resize_ffmpeg(image: &DynamicImage, dst_width: u32, dst_height: u32) -> DynamicImage {
    let src_width = image.width();
    let src_height = image.height();

    if src_width == dst_width && src_height == dst_height {
        return image.clone();
    }

    let format = get_pixel_format(image.color());

    let mut scaler = ScalingContext::get(format, src_width, src_height, format, dst_width, dst_height, Flags::FAST_BILINEAR).map_err(|e| e.to_string()).unwrap();

    let mut input_frame = Video::empty();
    input_frame.set_format(format);
    input_frame.set_width(src_width);
    input_frame.set_height(src_height);

    #[allow(unsafe_code)]
    unsafe {
        input_frame.alloc(format, src_width, src_height);
    }

    // Copy image data into the input frame

    let image_data = image.as_bytes();
    input_frame.data_mut(0).copy_from_slice(image_data);

    let mut output_frame = Video::empty();
    output_frame.set_format(format);
    output_frame.set_width(dst_width);
    output_frame.set_height(dst_height);

    #[allow(unsafe_code)]
    unsafe {
        output_frame.alloc(format, dst_width, dst_height);
    }

    // Scale input frame to YUV420P
    scaler.run(&input_frame, &mut output_frame).map_err(|e| e.to_string()).unwrap();

    DynamicImage::ImageRgb8(ImageBuffer::from_vec(dst_width, dst_height, output_frame.data(0).to_vec()).unwrap())
}

fn get_pixel_format(colour_type: ColorType) -> PixelFormat {
    match colour_type {
        image::ColorType::Rgb8 => PixelFormat::RGB24,
        image::ColorType::Rgba8 => PixelFormat::RGBA,
        image::ColorType::L8 => PixelFormat::GRAY8,
        image::ColorType::La8 => PixelFormat::GRAY8, // No direct mapping; consider conversion
        // Add other mappings as necessary
        _ => {
            println!("Unsupported ColorType {colour_type:?}, defaulting to RGB24");
            PixelFormat::RGB24
        }
    }
}
