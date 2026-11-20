#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use anyrender::{AntiAliasing, Filter, PaintScene, filters::FilterEffect};
use anyrender_vello_gpu::{ExternalTextures, ImageCache, ImageManager, VelloGpuScenePainter};
use kurbo::{Affine, Rect};
use peniko::{BlendMode, Color, Fill};
use vello_gpu::{RenderOptions, RenderSize, RenderTarget, RenderTargetConfig, Renderer, Scene};
use wgpu_context::{BufferRenderer, BufferRendererConfig, DeviceHandle};

const SIZE: u32 = 16;

fn assert_pixel(pixels: &[u8], x: usize, y: usize, expected: [u8; 4]) {
    let offset = (y * SIZE as usize + x) * 4;
    let actual = &pixels[offset..offset + 4];
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            actual.abs_diff(expected) <= 2,
            "pixel ({x}, {y}): {actual:?}, expected channel {expected}; RGBA {:?}",
            &pixels[offset..offset + 4],
        );
    }
}

#[test]
fn color_filters_render_in_order_with_stage_clamping_and_regions() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Ok(handle) = pollster::block_on(DeviceHandle::new_from_compatible_surface(
        instance, None, None, None,
    )) else {
        eprintln!("Skipping GPU color-filter pixel test: no Vulkan adapter/device available");
        return;
    };
    eprintln!("GPU color-filter adapter: {:?}", handle.adapter.get_info());
    let target = BufferRenderer::new(
        BufferRendererConfig {
            width: SIZE,
            height: SIZE,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        },
        handle.clone(),
        0,
    );
    let (mut renderer, mut resources) = Renderer::new(
        &handle.device,
        &handle.queue,
        &RenderTargetConfig {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: SIZE as u16,
            height: SIZE as u16,
        },
    );
    let depth = renderer.create_depth_texture_view(&RenderSize {
        width: SIZE as u16,
        height: SIZE as u16,
    });
    let mut cache = ImageCache::default();
    let mut textures = ExternalTextures::default();
    let canvas = Rect::new(0.0, 0.0, SIZE as f64, SIZE as f64);
    let cases = [
        (
            "brightness then invert",
            vec![FilterEffect::brightness(2.0), FilterEffect::invert(1.0)],
            canvas,
            Color::new([0.75, 0.25, 0.0, 0.5]),
            [0, 64, 128, 128],
        ),
        (
            "invert then brightness",
            vec![FilterEffect::invert(1.0), FilterEffect::brightness(2.0)],
            canvas,
            Color::new([0.75, 0.25, 0.0, 0.5]),
            [64, 128, 128, 128],
        ),
        (
            "grayscale",
            vec![FilterEffect::grayscale(1.0)],
            canvas,
            Color::new([1.0, 0.0, 0.0, 0.5]),
            [27, 27, 27, 128],
        ),
        (
            "filter region",
            vec![FilterEffect::brightness(2.0), FilterEffect::invert(1.0)],
            Rect::new(0.0, 0.0, 8.0, SIZE as f64),
            Color::new([0.75, 0.25, 0.0, 0.5]),
            [0, 64, 128, 128],
        ),
        (
            "empty filter",
            vec![],
            canvas,
            Color::new([0.75, 0.25, 0.0, 0.5]),
            [96, 32, 0, 128],
        ),
        (
            "unsupported-only filter",
            vec![FilterEffect::Tile],
            Rect::new(0.0, 0.0, 8.0, SIZE as f64),
            Color::new([0.75, 0.25, 0.0, 0.5]),
            [96, 32, 0, 128],
        ),
    ];
    let mut ordered_pixel = None;
    for (name, effects, region, color, expected) in cases {
        let mut scene = Scene::new(SIZE as u16, SIZE as u16);
        let mut painter = VelloGpuScenePainter::new(
            &mut scene,
            ImageManager::new(
                &mut resources,
                &handle.device,
                &handle.queue,
                &mut cache,
                &mut textures,
            ),
            &handle,
        );
        painter.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            1.0,
            Affine::IDENTITY,
            &canvas,
            Some(Arc::new(Filter::linear_list(effects, region))),
            None,
        );
        painter.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            color,
            None,
            &canvas,
            AntiAliasing::Disabled,
        );
        painter.pop_layer();
        // A draw after the pop also checks that internal filter/clip layers were balanced.
        painter.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::WHITE,
            None,
            &Rect::new(12.0, 12.0, 16.0, 16.0),
            AntiAliasing::Disabled,
        );
        drop(painter);
        let mut encoder = handle.device.create_command_encoder(&Default::default());
        renderer
            .render(
                &scene,
                &mut resources,
                &mut encoder,
                RenderTarget {
                    view: &target.target_texture_view(),
                    depth_view: Some(&depth),
                },
                textures.bindings(),
                RenderOptions::default(),
            )
            .unwrap();
        handle.queue.submit([encoder.finish()]);
        let mut pixels = vec![0; (SIZE * SIZE * 4) as usize];
        target.copy_texture_to_buffer(&mut pixels);
        let offset = (4 * SIZE as usize + 4) * 4;
        let pixel = &pixels[offset..offset + 4];
        eprintln!("{name}: premultiplied RGBA {pixel:?}, expected {expected:?}");
        assert_pixel(&pixels, 4, 4, expected);
        assert_pixel(&pixels, 14, 14, [255; 4]);
        if name == "brightness then invert" {
            ordered_pixel = Some(pixel.to_vec());
        } else if name == "invert then brightness" {
            assert_ne!(pixel, ordered_pixel.as_ref().unwrap());
        } else if name == "filter region" {
            assert_pixel(&pixels, 10, 4, [0; 4]);
        } else if name == "unsupported-only filter" {
            assert_pixel(&pixels, 10, 4, expected);
        }
    }
}
