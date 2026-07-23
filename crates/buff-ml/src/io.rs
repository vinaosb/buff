//! JSON serialization for [`Model`](crate::Model) save/load.
//!
//! The format is a JSON array of layer objects, each containing a `"kind"`
//! field and layer-specific parameter data. This module handles encoding
//! and decoding; the actual layer reconstruction is done by the caller.

use crate::error::{MlError, MlResult};
use crate::model::Model;
use buff_tensor::Tensor;
use std::fs;

/// Save the model to a JSON file.
pub(crate) fn save_model(model: &Model, path: &str) -> MlResult<()> {
    let layers_json: Vec<serde_json::Value> = model
        .layers
        .iter()
        .map(|layer| {
            let params = layer.parameters();
            let params_json: Vec<serde_json::Value> = params
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "shape": p.shape().as_slice().to_vec(),
                        "data": p.as_slice().to_vec(),
                    })
                })
                .collect();
            serde_json::json!({
                "kind": layer.layer_kind(),
                "params": params_json,
            })
        })
        .collect();

    let json = serde_json::to_string_pretty(&layers_json)
        .map_err(|e| MlError::Serialization(e.to_string()))?;
    fs::write(path, json)?;
    Ok(())
}

/// Load model parameters from a JSON file into an existing model.
///
/// The model's layer count and kinds must match the file.
pub(crate) fn load_model(model: &mut Model, path: &str) -> MlResult<()> {
    let json_str = fs::read_to_string(path)?;
    let layers_json: Vec<serde_json::Value> = serde_json::from_str(&json_str)
        .map_err(|e| MlError::Serialization(e.to_string()))?;

    if layers_json.len() != model.layers.len() {
        return Err(MlError::Serialization(format!(
            "layer count mismatch: model has {}, file has {}",
            model.layers.len(),
            layers_json.len()
        )));
    }

    for (i, (layer, lj)) in model.layers.iter_mut().zip(layers_json.iter()).enumerate() {
        let expected_kind = layer.layer_kind();
        let file_kind = lj["kind"]
            .as_str()
            .ok_or_else(|| MlError::Serialization(format!("layer {i}: missing kind")))?;
        if file_kind != expected_kind {
            return Err(MlError::Serialization(format!(
                "layer {i} kind mismatch: expected '{expected_kind}', got '{file_kind}'"
            )));
        }

        let params_json = lj["params"]
            .as_array()
            .ok_or_else(|| MlError::Serialization(format!("layer {i}: missing params")))?;

        let params: Vec<Tensor> = params_json
            .iter()
            .enumerate()
            .map(|(j, pj)| {
                let shape: Vec<usize> = pj["shape"]
                    .as_array()
                    .ok_or_else(|| {
                        MlError::Serialization(format!("layer {i} param {j}: missing shape"))
                    })?
                    .iter()
                    .filter_map(|v| v.as_u64().map(|u| u as usize))
                    .collect();
                let data: Vec<f32> = pj["data"]
                    .as_array()
                    .ok_or_else(|| {
                        MlError::Serialization(format!("layer {i} param {j}: missing data"))
                    })?
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                Tensor::from_vec(data, shape).map_err(|e| MlError::Serialization(e.to_string()))
            })
            .collect::<MlResult<Vec<Tensor>>>()?;

        layer.load_parameters(&params)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Linear;

    #[test]
    fn save_load_roundtrip_noop() {
        let mut model = Model::sequential(vec![Box::new(Linear::new(2, 3).unwrap())]);
        let dir = std::env::temp_dir();
        let path = dir.join("buff_ml_test_io_roundtrip.json");
        let path_str = path.to_str().unwrap_or("");

        model.save(path_str).unwrap();
        model.load(path_str).unwrap();

        let _ = fs::remove_file(path);
    }
}
