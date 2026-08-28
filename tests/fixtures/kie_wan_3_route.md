# Wan 3.0 - Video

## OpenAPI Specification

```yaml
openapi: 3.0.1
paths:
  /api/v1/jobs/createTask:
    post:
      requestBody:
        content:
          application/json:
            schema:
              type: object
              required: [model, input]
              properties:
                model:
                  type: string
                  enum: [wan/3-0-video]
                input:
                  type: object
                  properties:
                    prompt:
                      type: string
                      maxLength: 20000
                    first_frame_url: &media_url
                      type: object
                      properties: {}
                    last_frame_url: *media_url
                    reference_image_urls:
                      type: array
                      maxItems: 10
                      items: *media_url
                    resolution:
                      type: string
                      enum: [480P, 720P, 1080P]
                      default: 1080P
                    aspect_ratio:
                      type: string
                      enum: [adaptive, "16:9", "4:3", "1:1", "3:4", "9:16"]
                      default: adaptive
            examples:
              first-frame:
                value:
                  model: wan/3-0-video
                  input:
                    prompt: Animate this portrait
                    first_frame_url: https://example.test/portrait.png
                    resolution: 720P
                    aspect_ratio: adaptive
```
