# Grok Imagine Image 2.0 Segment Edit

## OpenAPI Specification

```yaml
openapi: 3.0.1
paths:
  /api/v1/jobs/createTask:
    post:
      operationId: grok-imagine-image-2-0-segment-edit
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
                    - grok-imagine-image-2-0/segment-edit
                  default: grok-imagine-image-2-0/segment-edit
                input:
                  type: object
                  required:
                    - prompt
                    - task_id
                  properties:
                    prompt:
                      type: string
                    task_id:
                      type: string
                    mask_indexs:
                      type: array
                      minItems: 1
                      items:
                        type: integer
                        minimum: 1
```
