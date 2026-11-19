#!/usr/bin/env python3
import hashlib
import json
import os
import sys
import time
import urllib.error
import urllib.request
import wave
import xml.etree.ElementTree as ET
import zipfile

DIARIZER = "diar_streaming_sortformer_4spk-v2.1"
AUDIO = "https://groups.inf.ed.ac.uk/ami/AMICorpusMirror/amicorpus/ES2004a/audio/ES2004a.Mix-Headset.wav"
AUDIO_SHA = "3e2560b19bee6952c7c7ce041b0f1ea8a7ea9468044c4eea79d2a2c67e24ab0f"
TRUTH = "https://raw.githubusercontent.com/pyannote/AMI-diarization-setup/67c2d539286e89f68952d5dcf83912bd9f01dfae/only_words/rttms/test/ES2004a.rttm"
TRUTH_SHA = "9869c6146c2fd9595403edb36c2caeda65c12ffa2c0af4ce48d6814b673fd5a9"
WORDS = "https://groups.inf.ed.ac.uk/ami/AMICorpusAnnotations/ami_public_manual_1.6.2.zip"
WORDS_SHA = "b56e5babb2496b8795deeeda7e71178d7fbc9963f94276cf2a3f4b56ebbc9f9d"
SECONDS = 420


def fetch(url, dest, sha256=None, tries=4):
    for attempt in range(tries):
        digest = hashlib.sha256()
        try:
            with urllib.request.urlopen(url, timeout=60) as response, open(dest + ".part", "wb") as out:
                while True:
                    block = response.read(1 << 20)
                    if not block:
                        break
                    digest.update(block)
                    out.write(block)
            break
        except (OSError, urllib.error.URLError):
            if attempt == tries - 1:
                raise
            time.sleep(5 * (attempt + 1))
    if sha256 and digest.hexdigest() != sha256:
        os.remove(dest + ".part")
        sys.exit(f"{os.path.basename(dest)}: checksum mismatch")
    os.replace(dest + ".part", dest)


def model(catalog, home, wanted):
    entry = next(m for m in catalog["models"] if m["id"] == wanted)
    file = entry["files"][entry["quant"]]
    speech = os.path.join(home, "models", "speech")
    os.makedirs(speech, exist_ok=True)
    url = f"https://huggingface.co/{entry['repository']}/resolve/{entry['revision']}/{file['filename']}"
    fetch(url, os.path.join(speech, file["filename"]), file["sha256"])
    return entry


def prepare_models(catalog_path, home, wanted):
    catalog = json.load(open(catalog_path))
    entry = model(catalog, home, wanted)
    model(catalog, home, DIARIZER)
    language = "en" if "en" in entry["languages"] else next(l for l in entry["languages"] if l.startswith("en"))
    json.dump({"id": wanted, "language": language}, open(os.path.join(home, "speech-model.json"), "w"))


def prepare_meeting(out):
    os.makedirs(out, exist_ok=True)
    whole = os.path.join(out, "whole.wav")
    fetch(AUDIO, whole, AUDIO_SHA)
    with wave.open(whole) as source:
        frames = source.readframes(SECONDS * source.getframerate())
        params = source.getparams()
    with wave.open(os.path.join(out, "meeting.wav"), "wb") as cut:
        cut.setparams(params)
        cut.writeframes(frames)
    os.remove(whole)
    fetch(TRUTH, os.path.join(out, "whole.rttm"), TRUTH_SHA)
    rows = []
    for line in open(os.path.join(out, "whole.rttm")):
        fields = line.split()
        start = float(fields[3])
        if start < SECONDS:
            fields[4] = f"{min(start + float(fields[4]), SECONDS) - start:.3f}"
            rows.append(" ".join(fields))
    open(os.path.join(out, "meeting.rttm"), "w").write("\n".join(rows) + "\n")
    annotations = os.path.join(out, "words.zip")
    fetch(WORDS, annotations, WORDS_SHA)
    said = []
    with zipfile.ZipFile(annotations) as archive:
        for speaker in "ABCD":
            for word in ET.fromstring(archive.read(f"words/ES2004a.{speaker}.words.xml")):
                start = word.get("starttime")
                if word.tag == "w" and start and float(start) < SECONDS and not word.get("punc") and not word.get("trunc"):
                    said.append((float(start), word.text))
    os.remove(annotations)
    open(os.path.join(out, "meeting.txt"), "w").write(" ".join(text for _, text in sorted(said)) + "\n")


def main():
    args = sys.argv[1:]
    if len(args) == 2 and args[0] == "--meeting":
        prepare_meeting(args[1])
    elif len(args) in (3, 4):
        catalog_path, home, wanted, *out = args
        prepare_models(catalog_path, home, wanted)
        if out:
            prepare_meeting(out[0])
    else:
        sys.exit("usage: speaker_check.py --meeting <out-dir> or <catalog.json> <leo-home> <model-id> [out-dir]")


if __name__ == "__main__":
    main()
