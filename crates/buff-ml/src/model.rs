//! Sequential model: a stack of [`Layer`]s with forward/backward/save/load.
//!
//! A [`Model`] owns a `Vec<Box<dyn Layer>>` and delegates forward pass,
//! gradient collection, parameter serialization, and save/load to the
//! individual layers.

use crate::autodiff::Var;
use crate::error::MlResult;
use crate::io;
use crate::layer::Layer;

/// A sequential neural network model.
///
/// Layers are executed in order during forward pass. The model provides
/// convenience methods for training (forward + backward) and persistence
/// (save/load as JSON).
pub struct Model {
    pub(crate) layers: Vec<Box<dyn Layer>>,
}

impl Model {
    /// Create a sequential model from a list of layers.
    pub fn sequential(layers: Vec<Box<dyn Layer>>) -> Self {
        Self { layers }
    }

    /// Forward pass: feed `x` through all layers sequentially.
    ///
    /// When `training` is true, stochastic layers like Dropout are active.
    pub fn forward(&self, x: Var, training: bool) -> MlResult<Var> {
        let mut y = x;
        for layer in &self.layers {
            y = layer.forward(y, training)?;
        }
        Ok(y)
    }

    /// Backward pass: seed the gradient at the loss output and propagate.
    ///
    /// This runs [`Var::backward`] on the loss, which fills the accumulated
    /// gradients in every parameter that `requires_grad`. After this call,
    /// [`Layer::collect_grads`] returns the gradients for the optimizer.
    pub fn backward(&self, loss: &Var) -> MlResult<()> {
        loss.backward()
    }

    /// Save the model (layer kinds + parameter tensors) to a JSON file.
    ///
    /// # Errors
    ///
    /// Returns [`MlError::Io`] on write failure or [`MlError::Serialization`]
    /// on JSON encoding failure.
    pub fn save(&self, path: &str) -> MlResult<()> {
        io::save_model(self, path)
    }

    /// Load a model from a JSON file, replacing all layer parameters.
    ///
    /// The file must contain the same number and types of layers as the
    /// current model.
    ///
    /// # Errors
    ///
    /// Returns [`MlError::Serialization`] if the JSON is malformed or the
    /// layer count/types don't match.
    pub fn load(&mut self, path: &str) -> MlResult<()> {
        io::load_model(self, path)
    }

    /// Mutable access to layers (for optimizers).
    pub(crate) fn layers_mut(&mut self) -> &mut [Box<dyn Layer>] {
        &mut self.layers
    }

    /// Immutable access to a specific layer by index (for inspection/testing).
    ///
    /// # Errors
    ///
    /// Returns `None` if `index` is out of bounds.
    pub fn get_layer(&self, index: usize) -> Option<&dyn Layer> {
        self.layers.get(index).map(|l| l.as_ref())
    }

    /// Number of layers.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Whether the model has no layers.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("layers", &self.layers.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autodiff::Tape;
    use crate::layer::{Linear, ReLU};
    use buff_tensor::Tensor;

    #[test]
    fn model_empty_forward() {
        let model = Model::sequential(vec![]);
        let tape = Tape::new();
        let x = tape
            .leaf(Tensor::from_vec(vec![1.0, 2.0], vec![2]).unwrap(), false)
            .unwrap();
        let y = model.forward(x, false).unwrap();
        assert_eq!(y.value().as_slice(), &[1.0, 2.0]);
    }

    #[test]
    fn model_single_linear_forward() {
        let model = Model::sequential(vec![Box::new(Linear::new(2, 3).unwrap())]);
        let tape = Tape::new();
        let x = tape
            .leaf(Tensor::from_vec(vec![1.0, 2.0], vec![1, 2]).unwrap(), false)
            .unwrap();
        let y = model.forward(x, false).unwrap();
        assert_eq!(y.value().shape().as_slice(), &[1, 3]);
    }

    #[test]
    fn model_debug_format() {
        let model = Model::sequential(vec![
            Box::new(Linear::new(2, 3).unwrap()),
            Box::new(ReLU::new()),
        ]);
        let dbg = format!("{:?}", model);
        assert!(dbg.contains("layers: 2"));
    }
}
