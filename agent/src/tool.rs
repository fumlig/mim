use schemars::{schema_for, JsonSchema};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Error, Value};

type ToolHandler = Box<dyn Fn(Value) -> Result<Value, Error> + Send + Sync>;

pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: Value, // json schema
    pub handler: ToolHandler,
}

/// Create a tool definition and handler from a function
pub fn function_tool<P, F, R>(name: String, description: String, f: F) -> Result<Tool, Error>
where
    P: JsonSchema + DeserializeOwned,
    F: Fn(P) -> Result<R, Error> + Send + Sync + 'static,
    R: Serialize,
{
    let schema = schema_for!(P);
    let parameters = serde_json::to_value(&schema)?;
    let handler: ToolHandler = Box::new(move |v: Value| {
        let params: P = serde_json::from_value(v)?;
        let output = f(params)?;

        serde_json::to_value(output)
    });

    Ok(Tool {
        name,
        description,
        parameters,
        handler,
    })
}
