//! Deterministic, stream-verified provisional .wpack framing.
//!
//! Format: eight-byte magic, big-endian u32 canonical manifest-transcript
//! length, transcript, then raw object bytes in canonical path order.
//! This is an unsigned transport envelope, NOT an installer, signature,
//! official publisher attestation, or proof that required media is present.
//! Callers must stage output and discard it on any error.

use std::io::{self, Read, Write};

use sha2::{Digest, Sha256};

use crate::manifest::{Manifest, ManifestError, Object, Origin, MAX_PACK_OBJECTS};

const MAGIC: &[u8; 8] = b"FWPACK01";
const TRANSCRIPT_PREFIX: &[u8] = b"foundation-wikipedia/manifest-identity/v1\0";
/// Bound manifest memory independently of object payload sizes.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum ContainerError {
    Io(io::Error),
    Manifest(ManifestError),
    InvalidHeader,
    InvalidTranscript,
    ResourceBudget,
    ObjectLength(String),
    ObjectDigest(String),
    TrailingBytes,
}

impl From<io::Error> for ContainerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ManifestError> for ContainerError {
    fn from(error: ManifestError) -> Self {
        Self::Manifest(error)
    }
}

fn read_u32<R: Read>(input: &mut R) -> Result<u32, ContainerError> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_u64<R: Read>(input: &mut R) -> Result<u64, ContainerError> {
    let mut bytes = [0; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_be_bytes(bytes))
}

fn read_field<R: Read>(input: &mut R) -> Result<String, ContainerError> {
    let length = read_u64(input)?;
    if length > 4096 {
        return Err(ContainerError::ResourceBudget);
    }
    let mut bytes = vec![0; length as usize];
    input.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| ContainerError::InvalidTranscript)
}

fn parse_transcript(bytes: &[u8]) -> Result<Manifest, ContainerError> {
    let mut input = io::Cursor::new(bytes);
    let mut prefix = vec![0; TRANSCRIPT_PREFIX.len()];
    input.read_exact(&mut prefix)?;
    if prefix != TRANSCRIPT_PREFIX {
        return Err(ContainerError::InvalidTranscript);
    }
    let version = read_u32(&mut input)?;
    let pack_id = read_field(&mut input)?;
    let project = read_field(&mut input)?;
    let snapshot = read_field(&mut input)?;
    let mut kind = [0];
    input.read_exact(&mut kind)?;
    let origin = match kind[0] {
        1 => Origin::Official {
            publisher: read_field(&mut input)?,
        },
        2 => Origin::Custom {
            definition_id: read_field(&mut input)?,
        },
        _ => return Err(ContainerError::InvalidTranscript),
    };
    let count = read_u64(&mut input)?;
    if count == 0 || count > MAX_PACK_OBJECTS as u64 {
        return Err(ContainerError::ResourceBudget);
    }
    let mut objects = Vec::new();
    for _ in 0..count {
        objects.push(Object {
            path: read_field(&mut input)?,
            sha256: read_field(&mut input)?,
            bytes: read_u64(&mut input)?,
        });
    }
    if input.position() != bytes.len() as u64 {
        return Err(ContainerError::InvalidTranscript);
    }
    let manifest = Manifest {
        version,
        pack_id,
        project,
        snapshot,
        origin,
        objects,
    };
    // Reject noncanonical object order, digest casing, or alternate encodings.
    if manifest.identity_transcript()? != bytes {
        return Err(ContainerError::InvalidTranscript);
    }
    Ok(manifest)
}

fn canonical_objects(manifest: &Manifest) -> Vec<&Object> {
    let mut objects: Vec<_> = manifest.objects.iter().collect();
    objects.sort_by(|a, b| a.path.cmp(&b.path));
    objects
}

struct HashingWriter<'a, W> {
    output: &'a mut W,
    remaining: u64,
    digest: Sha256,
}

impl<W: Write> Write for HashingWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object exceeds declared length",
            ));
        }
        let written = self.output.write(bytes)?;
        self.remaining -= written as u64;
        self.digest.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

/// Write a canonical envelope using a bounded streaming producer per object.
/// A failed producer leaves a partial envelope: publish only after Ok.
pub fn write_container<W: Write, F>(
    output: &mut W,
    manifest: &Manifest,
    mut produce: F,
) -> Result<(), ContainerError>
where
    F: FnMut(&Object, &mut dyn Write) -> io::Result<()>,
{
    let transcript = manifest.identity_transcript()?;
    if transcript.len() > MAX_MANIFEST_BYTES {
        return Err(ContainerError::ResourceBudget);
    }
    output.write_all(MAGIC)?;
    output.write_all(&(transcript.len() as u32).to_be_bytes())?;
    output.write_all(&transcript)?;
    for object in canonical_objects(manifest) {
        let mut writer = HashingWriter {
            output,
            remaining: object.bytes,
            digest: Sha256::new(),
        };
        produce(object, &mut writer)?;
        if writer.remaining != 0 {
            return Err(ContainerError::ObjectLength(object.path.clone()));
        }
        if !format!("{:x}", writer.digest.finalize()).eq_ignore_ascii_case(&object.sha256) {
            return Err(ContainerError::ObjectDigest(object.path.clone()));
        }
    }
    Ok(())
}

/// Validate every streamed object and reject appended data. This is read-only:
/// installers must stage and authenticate separately before activation.
pub fn verify_container<R: Read>(input: &mut R) -> Result<Manifest, ContainerError> {
    let mut magic = [0; 8];
    input.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(ContainerError::InvalidHeader);
    }
    let length = read_u32(input)? as usize;
    if length == 0 || length > MAX_MANIFEST_BYTES {
        return Err(ContainerError::ResourceBudget);
    }
    let mut transcript = vec![0; length];
    input.read_exact(&mut transcript)?;
    let manifest = parse_transcript(&transcript)?;
    let mut buffer = [0u8; 64 * 1024];
    for object in canonical_objects(&manifest) {
        let mut digest = Sha256::new();
        let mut remaining = object.bytes;
        while remaining > 0 {
            let size = remaining.min(buffer.len() as u64) as usize;
            input.read_exact(&mut buffer[..size])?;
            digest.update(&buffer[..size]);
            remaining -= size as u64;
        }
        if !format!("{:x}", digest.finalize()).eq_ignore_ascii_case(&object.sha256) {
            return Err(ContainerError::ObjectDigest(object.path.clone()));
        }
    }
    let mut extra = [0];
    loop {
        match input.read(&mut extra) {
            Ok(0) => break,
            Ok(_) => return Err(ContainerError::TrailingBytes),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(ContainerError::Io(error)),
        }
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            version: 1,
            pack_id: "physics".into(),
            project: "enwiki".into(),
            snapshot: "20261001".into(),
            origin: Origin::Custom {
                definition_id: "test".into(),
            },
            objects: vec![
                Object {
                    path: "media/figure.svg".into(),
                    sha256: format!("{:x}", Sha256::digest(b"<svg/>")),
                    bytes: 6,
                },
                Object {
                    path: "articles/1.cbor".into(),
                    sha256: format!("{:x}", Sha256::digest(b"article")),
                    bytes: 7,
                },
            ],
        }
    }

    fn envelope(manifest: &Manifest) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_container(&mut bytes, manifest, |object, output| {
            let payload: &[u8] = if object.path.starts_with("media/") {
                b"<svg/>"
            } else {
                b"article"
            };
            output.write_all(payload)
        })
        .unwrap();
        bytes
    }

    #[test]
    fn roundtrip_is_canonical_and_stream_verified() {
        let manifest = sample();
        let bytes = envelope(&manifest);
        assert_eq!(
            verify_container(&mut bytes.as_slice()).unwrap().pack_id,
            "physics"
        );
        let mut reordered = manifest.clone();
        reordered.objects.reverse();
        reordered.objects[0].sha256.make_ascii_uppercase();
        assert_eq!(envelope(&reordered), bytes);
    }

    #[test]
    fn rejects_truncation_corruption_and_appended_bytes() {
        let bytes = envelope(&sample());
        for cut in [0, 7, 11, bytes.len() - 1] {
            assert!(verify_container(&mut &bytes[..cut]).is_err());
        }
        let mut tampered = bytes.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(matches!(
            verify_container(&mut tampered.as_slice()),
            Err(ContainerError::ObjectDigest(_))
        ));
        let mut appended = bytes;
        appended.push(0);
        assert!(matches!(
            verify_container(&mut appended.as_slice()),
            Err(ContainerError::TrailingBytes)
        ));
    }

    #[test]
    fn rejects_wrong_producer_length_and_digest() {
        let mut bytes = Vec::new();
        let manifest = sample();
        let short = write_container(&mut bytes, &manifest, |_object, _out| Ok(()));
        assert!(matches!(short, Err(ContainerError::ObjectLength(_))));
        bytes.clear();
        let wrong = write_container(&mut bytes, &manifest, |object, out| {
            out.write_all(&vec![b'x'; object.bytes as usize])
        });
        assert!(matches!(wrong, Err(ContainerError::ObjectDigest(_))));
        bytes.clear();
        let long = write_container(&mut bytes, &manifest, |object, out| {
            out.write_all(&vec![b'x'; object.bytes as usize + 1])
        });
        assert!(matches!(long, Err(ContainerError::Io(_))));
    }

    #[test]
    fn rejects_forged_header_or_noncanonical_manifest() {
        let mut bytes = envelope(&sample());
        bytes[0] ^= 1;
        assert!(matches!(
            verify_container(&mut bytes.as_slice()),
            Err(ContainerError::InvalidHeader)
        ));
        let mut bytes = envelope(&sample());
        bytes[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            verify_container(&mut bytes.as_slice()),
            Err(ContainerError::ResourceBudget)
        ));
        let mut bytes = envelope(&sample());
        let header_end = 12 + TRANSCRIPT_PREFIX.len();
        bytes[header_end..header_end + 4].copy_from_slice(&2u32.to_be_bytes());
        assert!(matches!(
            verify_container(&mut bytes.as_slice()),
            Err(ContainerError::Manifest(ManifestError::Version))
        ));
    }
}
