# Domain Model

This document defines the core data structures and types for the **Listening Content Generation** bounded context.
It serves as the blueprint for the Rust implementation.

## Core Types

### ListeningSection

- **Type:** Enum

- **Variants:**

  - `Section1`: Transactional Conversation (2 speakers)
  - `Section2`: Guided Monologue (1 speaker)
  - `Section3`: Academic Discussion (2 speakers)
  - `Section4`: Academic Lecture (1 speaker)

### SpeakerRole

- **Type:** Enum

- **Variants:**

  - `Student`
  - `Professor`
  - `Clerk`
  - `Receptionist`
  - `Guide`
  - `Other(String)`

### Accent

- **Type:** Enum

- **Variants:**

  - `British`
  - `American`
  - `Australian`
  - `Canadian`
  - `NewZealand`

### Gender

- **Type:** Enum

- **Variants:**

  - `Male`
  - `Female`

## Value Objects

### SpeakerConfig

- **Description:** Configuration for a single speaker voice.

- **Fields:**

  - `name`: String (Internal identifier, e.g., "Speaker A")
  - `gender`: Gender
  - `accent`: Accent
  - `role`: SpeakerRole

### ScriptLine

- **Description:** A single line of dialogue or monologue.

- **Fields:**

  - `speaker_id`: String (Reference to a SpeakerConfig name)
  - `text`: String (The spoken content)
  - `start_time`: Duration (Optional, for alignment)
  - `end_time`: Duration (Optional, for alignment)

## Aggregates / Entities

### ListeningScript

- **Description:** The generated text content before audio synthesis.

- **Fields:**

  - `section`: ListeningSection
  - `topic`: String
  - `lines`: List of ScriptLine
  - `estimated_duration`: Duration

- **Invariants:**

  - Must have at least one line.
  - Speakers referenced in `lines` must exist in the request configuration.
  - Speaker count must match `section` rules.

### AudioTrack

- **Description:** The final audio output.

- **Fields:**

  - `format`: String (e.g., "mp3", "wav")
  - `duration`: Duration
  - `url`: String (Path or URL to the file)
  - `metadata`: Map from String to String

## Commands (DTOs)

### GenerationRequest

- **Description:** The input payload to trigger generation.

- **Fields:**

  - `section`: ListeningSection
  - `topic`: String

- **Validation:**

  - `topic` must not be empty.

- **Note:** Speaker configurations are automatically generated based on the selected `section` type.

## Results

### GenerationResult

- **Description:** The success response.

- **Fields:**

  - `request_id`: UUID
  - `script`: ListeningScript
  - `audio`: AudioTrack

### GenerationFailure

- **Description:** The error response.

- **Fields:**

  - `reason`: FailureReason (Enum)
  - `message`: String (User-friendly message)

### FailureReason

- **Type:** Enum

- **Variants:**

  - `InvalidConfiguration`: The request parameters violate section rules.
  - `ScriptGenerationError`: The LLM failed to produce a valid script.
  - `AudioSynthesisError`: The TTS service failed.
  - `SystemError`: Unexpected internal error.
