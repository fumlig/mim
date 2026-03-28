pub enum ResponseStreamEvent {
    TextDelta(String),
}

pub type ResponseStream<Error> =
    std::pin::Pin<Box<dyn futures::Stream<Item = Result<ResponseStreamEvent, Error>> + Send>>;

pub type ResponseResult<Error> = Result<ResponseStream<Error>, Error>;

pub trait Provider {
    type Error;

    fn create_response(
        &self,
        prompt: &str,
        model: &str,
    ) -> impl std::future::Future<Output = ResponseResult<Self::Error>> + Send;
}

#[cfg(feature = "openai")]
pub mod openai;
