# Experimental IFCCAD drawings in IFCX

OCS's **IFCCAD** format choice reads and writes CAD drawings in the experimental
IFCCAD profile of IFCX. Files keep the `.ifcx` extension. The choice does not
mean that OCS can open every IFCX document or BIM geometry representation.

buildingSMART's IFCX examples, such as Hello Wall, contain building-model
information and geometry. They do not contain an IFCCAD drawing that the current
OCS reader can open. This is an unsupported content type, not evidence that the
example is broken or has no geometry.

## Try the current drawing workflow

1. Open a DWG/DXF drawing or create a small drawing in OCS.
2. Keep the original and a separate DWG/DXF copy.
3. Choose **Save As → IFCCAD** and acknowledge the experimental-format warning.
4. Review any conversion losses listed below the alpha warning before proceeding.
5. Reopen the `.ifcx` file with the same OCS version and inspect the result.

The [IFCCAD Explorer](https://ifccad-explorer.open-aec.com/) provides drawing
examples and conversion inspection. Its current reader and examples may use a
different profile version from an installed OCS release. Use examples explicitly
qualified for the profile supported by that OCS build; the file extension alone
does not establish compatibility. See the
[IFCCAD project](https://github.com/OpenAEC-Foundation/ifccad) for profile contracts
and versioned source releases.

## Alpha compatibility

**For testing only.** The profile is under active development. A later OCS release
may not open a file saved today. Backward compatibility and automatic or manual
migration tools are not guaranteed. Do not use an experimental `.ifcx` drawing as
the only retained copy or for long-term archiving.

OCS asks for acknowledgement before the first interactive IFCCAD save in each
open drawing session. Reopening the drawing or restarting OCS asks again. The
Save As dialog also identifies the experimental status. The same dialog always explains alpha compatibility first and then lists any
conversion losses. Its save button confirms the displayed warning and losses together.
Autosave recovery and headless operations do not wait for this interactive modal;
the same alpha limitations still apply to all IFCCAD output.

This is the [IFCCAD alpha policy](https://github.com/OpenAEC-Foundation/ifccad/blob/main/docs/alpha-compatibility.md),
not a compatibility policy for every IFCX application or profile.

## Opening diagnostics

- **No IFCCAD drawing:** no supported CAD drawing content was found. BIM viewing,
  BIM-to-CAD conversion and projection generation need a separate future route.
- **Unsupported IFCX or IFCCAD profile version:** the file identifies a version
  this build does not support. Use the application version that created it or
  return to the original drawing. Updating OCS is not a guaranteed migration.
- **Other read failure:** malformed or unsupported input remains a read failure.

Technical reader details remain available in each error. Successful production
reads and strict drawing validation remain authoritative; explanatory inspection
of a failed input does not validate it as general IFCX or a valid BIM model.

## Future BIM workflows

Viewing BIM as a reference, generating plans/sections/elevations, semantic source
links and updating generated drawings are separate design work under
[OCS #763](https://github.com/HakanSeven12/OpenCADStudio/issues/763). The current
IFCCAD choice does not implement or promise those workflows.
