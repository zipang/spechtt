# OPENCODE / LINUX SPEECH TO TEXT WITH ANY BACKEND

A Rust widget that should integrate with any Linux system to capture the audio stream, send it to a local or remote Model, and paste the resul in the clipboard or inside a text input.

## LOCAL MODELS

## REMOTE MODELS PROVIDERS

### VERCEL AI GATEWAY

Vercel AI Gateway offers several streaming Speech transcription models with a unified API through their Vercel AI SDK (for Typescript): 

* mistral/voxtral-mini-transcribe-3
* spacexai/grok-stt

The list of models is activated on detection of the VERCEL_AI_GATEWAY_API_KEY environment variable. 

### MISTRAL AI

Mistral offers
