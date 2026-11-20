use anyrender::filters::{EdgeMode, Filter, FilterEffect, FilterInput, FilterSource};
use vello_common::filter_effects::FilterPrimitive;

/// Supports only single-input chains from SourceGraphic, with one region per chain.
/// Every primitive runs in sRGB; node colour spaces are ignored, so this is not general SVG parity.
/// Returns identity for unsupported sources (including SourceAlpha/BackgroundImage),
/// two-input nodes, invalid references, or cycles on the output chain.
pub(crate) fn convert_filter(filter: &Filter) -> Vec<vello_common::filter_effects::Filter> {
    let nodes = filter.nodes();
    let mut current = usize::from(filter.output().0);
    let mut visited = vec![false; nodes.len()];
    let mut filters = Vec::new();
    while let Some(node) = nodes.get(current) {
        if visited[current]
            || node.inputs.secondary.is_some()
            || matches!(
                node.effect,
                FilterEffect::Blend(_)
                    | FilterEffect::Composite(_)
                    | FilterEffect::DisplacementMap(_)
            )
        {
            return Vec::new();
        }
        visited[current] = true;
        if let Some(primitive) = convert_filter_effect(&node.effect) {
            filters.push(vello_common::filter_effects::Filter::from_primitive(
                primitive,
            ));
        }
        current = match &node.inputs.primary {
            Some(FilterInput::Source(FilterSource::SourceGraphic)) => break,
            Some(FilterInput::Source(_)) => return Vec::new(),
            Some(FilterInput::Result(id)) => usize::from(id.0),
            None if current == 0 => break,
            None => current - 1,
        };
        if current >= nodes.len() {
            return Vec::new();
        }
    }
    filters.reverse();
    filters
}

/// Applies one region to the whole chain: the output node's region.
/// Heterogeneous intermediate regions are not applied individually; identity has no region.
pub(crate) fn convert_filter_with_region(
    filter: &Filter,
) -> (
    Vec<vello_common::filter_effects::Filter>,
    Option<kurbo::Rect>,
) {
    let filters = convert_filter(filter);
    let region = (!filters.is_empty())
        .then(|| {
            filter
                .nodes()
                .get(usize::from(filter.output().0))
                .map(|node| node.region)
        })
        .flatten();
    (filters, region)
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
    use anyrender::filters::{FilterColorSpace, FilterId, FilterInput, FilterInputs, FilterSource};
    use kurbo::{Affine, Rect};
    use vello_common::filter::PreparedFilter;

    fn graphic() -> FilterInput {
        FilterInput::Source(FilterSource::SourceGraphic)
    }

    fn assert_same_chain(filter: &Filter, effects: impl IntoIterator<Item = FilterEffect>) {
        let expected: Vec<_> = effects
            .into_iter()
            .filter_map(|effect| convert_filter_effect(&effect))
            .map(vello_common::filter_effects::Filter::from_primitive)
            .collect();
        assert_eq!(convert_filter(filter), expected);
    }

    #[test]
    fn linear_list_is_unchanged() {
        let effects = [
            FilterEffect::Offset(kurbo::Vec2::new(4.0, 0.0)),
            FilterEffect::brightness(2.0),
            FilterEffect::invert(1.0),
        ];
        assert_same_chain(&Filter::linear_list(effects.clone(), Rect::ZERO), effects);
    }

    #[test]
    fn chain_region_preserves_linear_lists_and_identity() {
        let filter = Filter::linear_list(
            [
                FilterEffect::Offset(kurbo::Vec2::new(4.0, 0.0)),
                FilterEffect::invert(1.0),
            ],
            Rect::new(0.0, 0.0, 16.0, 16.0),
        );
        let region = convert_filter_with_region(&filter).1.unwrap();
        assert!(filter.nodes().iter().all(|node| node.region == region));
        assert!(
            convert_filter_with_region(&Filter::new(Rect::ZERO))
                .1
                .is_none()
        );
        assert!(
            convert_filter_with_region(&Filter::single(FilterEffect::Tile, Rect::ZERO))
                .1
                .is_none()
        );
    }

    #[test]
    fn source_graphic_resets_the_chain() {
        let mut filter = Filter::new(Rect::ZERO);
        filter.add_with_color_space(
            FilterEffect::Offset(kurbo::Vec2::new(4.0, 0.0)),
            FilterInputs::single(graphic()),
            FilterColorSpace::Srgb,
        );
        filter.add_with_color_space(
            FilterEffect::invert(1.0),
            FilterInputs::single(graphic()),
            FilterColorSpace::Srgb,
        );
        assert_same_chain(&filter, [FilterEffect::invert(1.0)]);
    }

    #[test]
    fn explicit_node_and_previous_result_chain() {
        let mut filter = Filter::new(Rect::ZERO);
        let first = filter.add_with_color_space(
            FilterEffect::brightness(2.0),
            FilterInputs::NONE,
            FilterColorSpace::Srgb,
        );
        filter.add_with_color_space(
            FilterEffect::Offset(kurbo::Vec2::new(4.0, 0.0)),
            FilterInputs::NONE,
            FilterColorSpace::Srgb,
        );
        filter.add_with_color_space(
            FilterEffect::invert(1.0),
            FilterInputs::single(FilterInput::Result(first)),
            FilterColorSpace::Srgb,
        );
        filter.add_with_color_space(
            FilterEffect::opacity(0.5),
            FilterInputs::NONE,
            FilterColorSpace::Srgb,
        );
        assert_same_chain(
            &filter,
            [
                FilterEffect::brightness(2.0),
                FilterEffect::invert(1.0),
                FilterEffect::opacity(0.5),
            ],
        );
    }

    #[test]
    fn invalid_and_cyclic_inputs_are_identity() {
        for id in [FilterId(99), FilterId(0)] {
            let mut filter = Filter::new(Rect::ZERO);
            filter.add_with_color_space(
                FilterEffect::invert(1.0),
                FilterInputs::single(FilterInput::Result(id)),
                FilterColorSpace::Srgb,
            );
            assert!(convert_filter(&filter).is_empty(), "input {id:?}");
        }
        let mut filter = Filter::new(Rect::ZERO);
        filter.add_with_color_space(
            FilterEffect::brightness(2.0),
            FilterInputs::single(FilterInput::Result(FilterId(1))),
            FilterColorSpace::Srgb,
        );
        filter.add_with_color_space(
            FilterEffect::invert(1.0),
            FilterInputs::single(FilterInput::Result(FilterId(0))),
            FilterColorSpace::Srgb,
        );
        assert!(convert_filter(&filter).is_empty());
    }

    #[test]
    fn unsupported_sources_and_two_inputs_are_identity() {
        for source in [FilterSource::SourceAlpha, FilterSource::BackgroundImage] {
            let mut filter = Filter::new(Rect::ZERO);
            filter.add_with_color_space(
                FilterEffect::invert(1.0),
                FilterInputs::single(FilterInput::Source(source)),
                FilterColorSpace::Srgb,
            );
            assert!(convert_filter(&filter).is_empty());
        }
        let mut filter = Filter::new(Rect::ZERO);
        filter.add_with_color_space(
            FilterEffect::invert(1.0),
            FilterInputs::NONE,
            FilterColorSpace::Srgb,
        );
        filter.add_with_color_space(
            FilterEffect::Blend(peniko::Mix::Multiply),
            FilterInputs::dual(graphic(), graphic()),
            FilterColorSpace::Srgb,
        );
        assert!(convert_filter(&filter).is_empty());
    }

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
