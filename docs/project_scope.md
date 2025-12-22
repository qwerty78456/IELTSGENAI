# Project Scope & Boundaries

This document explicitly defines the boundaries of the **VMQ MVP** project.
It serves as a contract to prevent scope creep and ensure focus on the core value proposition.

## Project Vision

To provide IELTS teachers with an endless supply of high-quality, realistic listening practice materials, customized to specific topics, eliminating the need to search for or manually record content.

## Bounded Context

- **Name:** Listening Content Generation

- **Responsibility:**
  - Taking a configuration (Topic, Section, Speakers).
  - Generating a valid IELTS-style script.
  - Synthesizing realistic audio.

- **Everything else is external to this context.**

## In-Scope Features (MVP)

### 1. Script Generation

- **Section 1:** Transactional dialogues (e.g., booking a hotel).
- **Section 2:** Guided monologues (e.g., tour guide speech).
- **Section 3:** Academic discussions (e.g., student & tutor).
- **Section 4:** Academic lectures (e.g., university professor).
- **Constraint:** Scripts must adhere to the strict structural rules of each section.

### 2. Audio Synthesis

- **Multi-Speaker Support:** Ability to distinctively render different voices in the same track.
- **Accent Variety:** Support for British, American, and Australian accents.
- **Pacing Control:** Speech rate appropriate for the target section (e.g., slower for Section 1, faster for Section 4).

### 3. Teacher Configuration

- **Topic Selection:** Free-text input for the subject matter.
- **Speaker Customization:** Selection of gender and role.

## Out-of-Scope Features (Explicit)

### 1. Assessment & Grading

- **Excluded:** The system will **NOT** grade student answers.
- **Excluded:** The system will **NOT** provide feedback on student performance.
- **Reason:** This is a content generation tool, not a testing platform.

### 2. Question Generation

- **Excluded:** The system will **NOT** generate multiple-choice, fill-in-the-blank, or matching questions.
- **Reason:** Generating high-quality, unambiguous IELTS questions is a separate, complex domain. The MVP focuses solely on the *listening passage*.

### 3. Exam Simulation

- **Excluded:** The system will **NOT** simulate a full timed exam environment for students.
- **Reason:** The target user is the **Teacher**, not the Student.

### 4. User Management

- **Excluded:** Complex role-based access control, payment processing, or social login.
- **Reason:** MVP will run locally or as a simple hosted tool.

## MVP Success Criteria

- A teacher can generate a valid Section 3 audio track about "Climate Change" in under 10 minutes.
- The generated audio sounds natural enough to be used in a classroom setting.
- The script follows the structural conventions of IELTS (e.g., turn-taking, vocabulary).
