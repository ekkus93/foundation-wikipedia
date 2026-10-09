# Fixture provenance

Every fixture must have a `<filename>.provenance.json` sidecar recording source type, license and SHA-256 of its exact bytes. Wikimedia fixtures additionally record official source URL, project, page ID and revision ID. Missing or mismatched provenance must fail validation.
