# Google - Nano Banana 2

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
              required:
                - model
                - input
              properties:
                model:
                  type: string
                  enum:
                    - nano-banana-2
                input:
                  type: object
                  required:
                    - prompt
                  properties:
                    prompt:
                      type: string
                      maxLength: 20000
                      description: A text description of the image to generate.
                    image_input:
                      type: array
                      maxItems: 14
                      items:
                        type: string
                        format: uri
                    aspect_ratio:
                      type: string
                      enum:
                        - '1:1'
                        - '4:3'
                        - '16:9'
                        - auto
                    resolution:
                      type: string
                      enum:
                        - 1K
                        - 2K
                        - 4K
                    output_format:
                      type: string
                      enum:
                        - png
                        - jpg
```
