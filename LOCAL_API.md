# Local STT API Documentation

The Local Speech-to-Text (STT) API allows you to expose Handy's transcription capabilities as a local web service. It is designed to be compatible with the OpenAI and Groq transcription API formats.

## Configuration

You can manage the Local API in the application settings:

1. Open **Settings**.
2. Navigate to the **Advanced** tab.
3. Find the **Local API** section.
4. **Enable Local API**: Toggle the switch to start the server.
5. **Local API Port**: Specify the port you want the server to listen on (default is `5500`).

> [!NOTE]
> The server listens on `0.0.0.0`, making it accessible from other devices in your local network if your firewall allows it.

## API Endpoint

### Transcribe Audio

`POST /v1/audio/transcriptions`

Transcribes the uploaded audio file using the currently active model in Handy.

#### Request Headers

- `Content-Type: multipart/form-data`

#### Request Body (Multipart)

| Field                       | Type   | Required | Status          | Description                                                                                    |
| :-------------------------- | :----- | :------- | :-------------- | :--------------------------------------------------------------------------------------------- |
| `file`                      | file   | Yes      | **Functional**  | The audio file to transcribe (currently only supports **.mp3**).                               |
| `model`                     | string | No       | _Compatibility_ | Ignored. Handy always uses its currently active model selected in UI.                          |
| `response_format`           | string | No       | **Functional**  | Can be `json` (default) or `verbose_json`.                                                     |
| `timestamp_granularities[]` | string | No       | **Functional**  | Set to `segment` to include segment-level timestamps when `response_format` is `verbose_json`. |

#### Response Format

**Standard JSON (`response_format: json`)**

```json
{
  "text": "The transcribed text content."
}
```

**Verbose JSON (`response_format: verbose_json`)**

| Field      | Type   | Status         | Description                                                                    |
| :--------- | :----- | :------------- | :----------------------------------------------------------------------------- |
| `text`     | string | **Functional** | The full transcribed text content.                                             |
| `segments` | array  | **Functional** | List of transcription segments (requires `timestamp_granularities[]=segment`). |

**Segment Object Fields:**

| Field               | Type    | Status         | Description                                                         |
| :------------------ | :------ | :------------- | :------------------------------------------------------------------ |
| `start`             | float   | **Functional** | Start time of the segment in seconds (rounded to 2 decimal places). |
| `end`               | float   | **Functional** | End time of the segment in seconds (rounded to 2 decimal places).   |
| `text`              | string  | **Functional** | Text content of the segment.                                        |
| `id`                | integer | **Functional** | Auto-incrementing index starting from 0.                            |
| `seek`              | integer | _Fixed (0)_    | Compatibility placeholder.                                          |
| `tokens`            | array   | _Fixed ([])_   | Compatibility placeholder.                                          |
| `temperature`       | float   | _Fixed (0.0)_  | Compatibility placeholder.                                          |
| `avg_logprob`       | float   | _Fixed (0.0)_  | Compatibility placeholder.                                          |
| `compression_ratio` | float   | _Fixed (0.0)_  | Compatibility placeholder.                                          |
| `no_speech_prob`    | float   | _Fixed (0.0)_  | Compatibility placeholder.                                          |

Example verbose response:

```json
{
  "text": "The transcribed text content.",
  "segments": [
    {
      "start": 0.0,
      "end": 2.5,
      "text": "The transcribed text",
      "id": 0,
      "seek": 0,
      "tokens": [],
      "temperature": 0.0,
      "avg_logprob": 0.0,
      "compression_ratio": 0.0,
      "no_speech_prob": 0.0
    }
  ]
}
```

## Usage Example (cURL)

```bash
curl http://localhost:5500/v1/audio/transcriptions \
  -H "Content-Type: multipart/form-data" \
  -F file="@/path/to/your/audio.mp3" \
  -F response_format="verbose_json" \
  -F "timestamp_granularities[]=segment"
```

## Model Recommendations & Known Issues

### Known Issues

- **Audio Format Limitation**: Currently, the API only supports **.mp3** files. Support for additional formats (e.g., .m4a, .wav, .flac) is planned for the future. **Pull Requests (PRs) from the community are highly welcome to help implement broader format support!**

- **Hallucinations**: When using smaller models (like Whisper `small`), the model may occasionally append non-existent phrases at the end of the transcription, such as "谢谢大家" (Thank you everyone) or "字幕组" (Subtitle group). This typically happens during silent segments or at the very end of an audio file.

### Recommended Models

For the best balance of speed and accuracy, we recommend:

- **Chinese (中文)**: **Whisper Turbo** is currently the optimal choice for high-quality Chinese transcription.
- **English**: **Parakeet V3** is recommended for English transcription due to its exceptional processing speed.

## Troubleshooting

- **Address already in use**: If you see an error in the logs saying the address is in use, try changing the port in settings.
- **Firewall**: Ensure your system's firewall allows incoming connections on the chosen port if you plan to access the API from other devices.
