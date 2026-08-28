# Omnihuman 1.5

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
                  enum: [omnihuman-1-5]
                input:
                  type: object
                  required: [image_url, audio_url]
                  properties:
                    image_url:
                      type: string
                      format: uri
                    audio_url:
                      type: string
                      format: uri
                    prompt:
                      type: string
                      maxLength: 300
                    output_resolution:
                      type: string
                      enum: ["720", "1080"]
                      default: "1080"
            example:
              model: omnihuman-1-5
              input:
                image_url: https://example.test/portrait.png
                audio_url: https://example.test/voice.mp3
                prompt: A person speaking naturally.
                output_resolution: "1080"
```
