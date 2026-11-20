use anyrender::filters::{EdgeMode, Filter, FilterEffect};
use vello_common::filter_effects::FilterPrimitive;

pub(crate) fn convert_filter(filter: &Filter) -> Vec<vello_common::filter_effects::Filter> {
    filter
        .nodes()
        .iter()
        .filter_map(|node| convert_filter_effect(&node.effect))
        .map(vello_common::filter_effects::Filter::from_primitive)
        .collect()
}

pub(crate) fn convert_filter_effect(effect: &FilterEffect) -> Option<FilterPrimitive> {
    Some(match effect {
        FilterEffect::Flood(color) => FilterPrimitive::Flood { color: *color },
        FilterEffect::GaussianBlur(blur) => FilterPrimitive::GaussianBlur {
            std_deviation_x: blur.std_deviation_x,
            std_deviation_y: blur.std_deviation_y,
            edge_mode: convert_edge_mode(blur.edge_mode),
        },
        // Vello has one standard deviation for both axes.
        FilterEffect::DropShadow(shadow) if shadow.std_deviation_x != shadow.std_deviation_y => {
            return None;
        }
        FilterEffect::DropShadow(shadow) => FilterPrimitive::DropShadow {
            dx: shadow.dx,
            dy: shadow.dy,
            std_deviation: shadow.std_deviation_x,
            color: shadow.color,
            edge_mode: convert_edge_mode(shadow.edge_mode),
        },
        FilterEffect::Offset(offset) => FilterPrimitive::Offset {
            dx: offset.x as f32,
            dy: offset.y as f32,
        },
        FilterEffect::ColorMatrix(matrix) => FilterPrimitive::ColorMatrix { matrix: matrix.0 },
        FilterEffect::Blend(_mode) => return None, //FilterPrimitive::Blend { mode: *mode },
        FilterEffect::ComponentTransfer(_component_transfer_filter) => return None,
        FilterEffect::Composite(_composite_operator) => return None,
        FilterEffect::Morphology(_morphology_filter) => return None,
        FilterEffect::ConvolveMatrix(_convolution_kernel) => return None,
        FilterEffect::Turbulence(_turbulence_filter) => return None,
        FilterEffect::DisplacementMap(_displacement_map_filter) => return None,
        FilterEffect::Image(_external_image_source) => return None,
        FilterEffect::Tile => return None,
        FilterEffect::DiffuseLighting(_diffuse_lighting_filter) => return None,
        FilterEffect::SpecularLighting(_specular_lighting_filter) => return None,
    })
}

fn convert_edge_mode(edge_mode: EdgeMode) -> vello_common::filter_effects::EdgeMode {
    match edge_mode {
        EdgeMode::Duplicate => vello_common::filter_effects::EdgeMode::Duplicate,
        EdgeMode::Wrap => vello_common::filter_effects::EdgeMode::Wrap,
        EdgeMode::Mirror => vello_common::filter_effects::EdgeMode::Mirror,
        EdgeMode::None => vello_common::filter_effects::EdgeMode::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Affine, Rect};
    use vello_common::filter::PreparedFilter;

    #[test]
    fn css_color_matrices_are_preserved() {
        for effect in [
            FilterEffect::grayscale(0.4),
            FilterEffect::sepia(0.4),
            FilterEffect::saturate(0.4),
            FilterEffect::hue_rotate(0.4),
            FilterEffect::invert(0.4),
            FilterEffect::opacity(0.4),
            FilterEffect::brightness(0.4),
            FilterEffect::contrast(0.4),
        ] {
            let FilterEffect::ColorMatrix(expected) = &effect else {
                panic!()
            };
            let converted = convert_filter(&Filter::single(effect.clone(), Rect::ZERO));
            let PreparedFilter::ColorMatrix(actual) =
                PreparedFilter::new(&converted[0], &Affine::IDENTITY)
            else {
                panic!()
            };
            assert_eq!(actual.matrix, expected.0);
        }
    }

    #[test]
    fn nested_filters_apply_in_order_and_clamp_each_stage() {
        let filter = Filter::linear_list(
            [FilterEffect::brightness(2.0), FilterEffect::invert(1.0)],
            Rect::ZERO,
        );
        let mut filters = convert_filter(&filter);
        let outer = filters.pop().unwrap();
        let mut stack = vec![outer];
        stack.extend(filters.into_iter().rev());
        let mut pixel = [0.75, 0.25, 0.0, 1.0];
        while let Some(filter) = stack.pop() {
            let PreparedFilter::ColorMatrix(matrix) =
                PreparedFilter::new(&filter, &Affine::IDENTITY)
            else {
                panic!()
            };
            pixel = matrix.apply_premultiplied(pixel);
        }
        assert_eq!(pixel, [0.0, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn empty_and_unsupported_filters_do_not_create_layers() {
        assert!(convert_filter(&Filter::new(Rect::ZERO)).is_empty());
        let filter = Filter::linear_list(
            [
                FilterEffect::Tile,
                FilterEffect::invert(1.0),
                FilterEffect::Tile,
            ],
            Rect::ZERO,
        );
        assert_eq!(convert_filter(&filter).len(), 1);
    }
}
