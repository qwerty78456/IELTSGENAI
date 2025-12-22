# Ubiquitous Language Dictionary

This document defines the **Ubiquitous Language** for the **Listening Content Generation** bounded context.
These terms must be used consistently across all communication, documentation, and code.

## Core Domain Concepts

### ListeningSection

- **Definition:** One of the four distinct parts of an IELTS listening practice session.

- **Allowed Values:** `1`, `2`, `3`, `4` (Integer).

- **Strict Rules:**

  - **Section 1:** Transactional conversation (2 speakers). Everyday context.
  - **Section 2:** Guided monologue (1 speaker). General context.
  - **Section 3:** Academic discussion (2 speakers). Education/training context.
  - **Section 4:** Academic lecture (1 speaker). University context.

- **Forbidden Synonyms:** Part, Chapter, Module.

### GenerationRequest

- **Definition:** The command object initiated by a teacher to start the content generation process.

- **Context:** Contains the `GenerationConfig` and `SpeakerConfig`.

- **Invariant:** Must specify a target `ListeningSection`.

### GenerationConfig

- **Definition:** The set of parameters controlling the generation logic.

- **Attributes:** Topic, Target Duration.

### Speaker

- **Definition:** A distinct voice participant in the listening content.

- **Context:** Identified by a unique name or role (e.g., "Student", "Receptionist") within a script.

### SpeakerConfig

- **Definition:** The configuration defining a `Speaker`'s characteristics.

- **Attributes:** Gender, Accent (e.g., British, Australian, American), Role.

### ListeningScript

- **Definition:** The text representation of the generated audio content.

- **Structure:** An ordered sequence of lines, each attributed to a specific `Speaker`.

- **Invariant:** Must conform to the structure of the requested `ListeningSection`.

### AudioTrack

- **Definition:** The binary audio output generated from a `ListeningScript`.

- **Context:** The final artifact delivered to the teacher.

### GenerationResult

- **Definition:** The successful outcome of a `GenerationRequest`.

- **Contents:** Contains both the `ListeningScript` and the `AudioTrack`.

### GenerationFailure

- **Definition:** A domain-specific error indicating why a request could not be fulfilled.

- **Context:** Must be readable by a teacher (e.g., "Section 1 requires exactly 2 speakers").

- **Forbidden:** Generic HTTP errors or stack traces in isolation.

### ScriptValidation

- **Definition:** The domain service or process that verifies a `ListeningScript` against IELTS rules *before* audio generation.

- **Rules:** Checks for correct speaker count, appropriate tone, and section-specific constraints.

### AudioGeneration

- **Definition:** The process of synthesizing voice audio from a validated `ListeningScript`.

## Forbidden Terminology

| Forbidden Term | Use Instead | Reason |
| :--- | :--- | :--- |
| **Exam / Test** | **Practice Material** | We are not generating official exams. |
| **Question** | **(Out of Scope)** | This MVP does not generate questions. |
| **Grader** | **(Out of Scope)** | This MVP does not grade students. |
| **User** | **Teacher** | "Teacher" is the specific persona we serve. |
