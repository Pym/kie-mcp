# Kling 3.0

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
                    - kling-3.0/video
                input:
                  type: object
                  required:
                    - prompt
                    - mode
                  properties:
                    prompt:
                      type: string
                      minLength: 1
                      maxLength: 2500
                    mode:
                      type: string
                      enum:
                        - std
                        - pro
                    multi_prompt:
                      type: array
                      maxItems: 6
                      items:
                        type: object
                        required:
                          - prompt
                          - duration
                        properties:
                          prompt:
                            type: string
                            minLength: 1
                            maxLength: 512
                          duration:
                            type: integer
                            minimum: 1
                            maximum: 15
                    kling_elements:
                      type: array
                      maxItems: 3
                      items:
                        type: object
                        required:
                          - name
                          - element_input_urls
                        properties:
                          name:
                            type: string
                            minLength: 1
                          element_input_urls:
                            type: array
                            minItems: 1
                            maxItems: 4
                            items:
                              type: string
                              format: uri
```
