use object_store::aws::AmazonS3Builder;

pub fn s3_builder_from_cli(
    bucket: &str,
    region: Option<&str>,
    endpoint: Option<&str>,
) -> AmazonS3Builder {
    let mut s3_builder = AmazonS3Builder::from_env().with_bucket_name(bucket);

    if let Some(region) = region {
        s3_builder = s3_builder.with_region(region);
    }
    if let Some(endpoint) = endpoint {
        s3_builder = s3_builder.with_endpoint(endpoint);
    }

    s3_builder
}
