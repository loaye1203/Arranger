# Arranger command protocol

`arranger describe` lists the compiled capabilities. The request envelope and command parameters are defined in the [version 1 JSON Schema](api-v1.schema.json).

`arranger run REQUESTS.jsonl` executes one UTF-8 JSON request per line and writes one JSON response per line. Pass `-` to read from standard input. Requests use the following envelope.

```json
{"protocol_version":1,"request_id":1,"command":{"type":"describe"}}
```

`arranger gui --open FILE` opens an editable document. `arranger serve --endpoint ENDPOINT` starts a local document session without a window. Use `arranger send ENDPOINT REQUEST.json` to submit individual requests and `arranger stop ENDPOINT` to end the session. The endpoint file belongs to that local session.

[The example generator](../examples/v1_design.py) shows how to submit an operation batch, save the editable document and export SVG, PNG and PDF. See the schema for the CDR preflight and export options.

