//! Chart DTO adapter over the independent compatibility renderers.
use chartgen_model::{Result, Scene};
pub use figkit_render::legacy::{TypstLayer, TypstPlan, typst, typst_escape};
pub trait Renderer {
    fn render(&self, scene: &Scene) -> Result<String>;
}
macro_rules! adapter {
    ($name:ident) => {
        #[derive(Default)]
        pub struct $name;
        impl Renderer for $name {
            fn render(&self, scene: &Scene) -> Result<String> {
                figkit_render::legacy::Renderer::render(
                    &figkit_render::legacy::$name,
                    &scene.to_legacy(),
                )
            }
        }
    };
}
adapter!(SvgRenderer);
adapter!(CompatibilitySvgRenderer);
adapter!(PgfRenderer);
adapter!(TypstRenderer);
/// Chart-owned DTO conversion preserves datum associations outside shared rendering.
pub mod marks {
    use chartgen_model::{Primitive, Result};
    fn from_legacy(
        value: figkit_model::legacy::Primitive,
        association: &Option<(String, String)>,
    ) -> Result<Primitive> {
        let mut primitive: Primitive = serde_json::from_value(
            serde_json::to_value(value)
                .map_err(|e| chartgen_model::Error::new("render", e.to_string()))?,
        )
        .map_err(|e| chartgen_model::Error::new("render", e.to_string()))?;
        primitive.meta_mut().value_of = association.clone();
        Ok(primitive)
    }
    pub fn symbol(
        href: &str,
        center: &chartgen_model::Point,
        size: f64,
        paint: &chartgen_model::Paint,
        meta: &chartgen_model::Metadata,
    ) -> Result<Primitive> {
        from_legacy(
            figkit_render::legacy::marks::symbol(href, center, size, paint, &meta.to_legacy())?,
            &meta.value_of,
        )
    }
    pub fn lower(p: &Primitive) -> Result<Option<Vec<Primitive>>> {
        figkit_render::legacy::marks::lower(&p.to_legacy())?
            .map(|v| {
                v.into_iter()
                    .map(|v| from_legacy(v, &p.meta().value_of))
                    .collect()
            })
            .transpose()
    }
    pub fn wedge(p: &Primitive) -> Result<Option<Primitive>> {
        figkit_render::legacy::marks::wedge(&p.to_legacy())?
            .map(|v| from_legacy(v, &p.meta().value_of))
            .transpose()
    }
}
