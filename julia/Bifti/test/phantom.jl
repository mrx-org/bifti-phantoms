const UNITS_JSON = """{"gyro": "MHz/T", "B0": "T", "T1": "s", "T2": "s", "T2'": "s",
    "ADC": "10^-3 mm^2/s", "dB0": "Hz", "B1+": "rel", "B1-": "rel"}"""

phantom_json(extra="", tissue="\"T1\": 1.5") = """{
    "\$schema": "bifti-phantom-v1.schema.json",
    "units": $UNITS_JSON,
    "system": {"gyro": 42.5764, "B0": 3.0},
    $extra
    "tissues": {"gm": {"density": "x.nii.gz[0]", $tissue}}
}"""

parse_phantom(json) = Bifti.BiftiPhantom(Bifti.JSON.parse(json))

function round_trip(phantom)
    path = joinpath(mktempdir(), "phantom.json")
    write_bifti(path, phantom)
    return read_bifti(path)
end

@testset "phantom config" begin
    @testset "patient positions" begin
        # FFS is the identity, and an unpositioned phantom is never transformed.
        @test scanner_matrix(Bifti.FFS) == [1 0 0; 0 1 0; 0 0 1]
        @test scanner_matrix(BiftiPhantom()) == scanner_matrix(Bifti.FFS)
        for position in instances(PatientPosition)
            P = scanner_matrix(position)
            # Every position is a proper rotation ...
            @test P * P' == [1 0 0; 0 1 0; 0 0 1]
            @test det(P) ≈ 1
            # ... and head first maps superior onto -Z, i.e. into the bore.
            @test P[3, 3] == (startswith(string(position), "HF") ? -1 : 1)
            # Codes round-trip through their DICOM spelling.
            @test parse(PatientPosition, string(position)) == position
        end
        # HFS is FFS turned by 180° about the vertical axis.
        @test scanner_matrix(Bifti.HFS) == [-1 0 0; 0 1 0; 0 0 -1]
        # Codes are case-sensitive, and non-MR DICOM codes are not supported.
        @test_throws ArgumentError parse(PatientPosition, "hfs")
        @test_throws ArgumentError parse(PatientPosition, "SITTING")
    end

    @testset "patient is optional and round-trips" begin
        @test isnothing(parse_phantom(phantom_json()).patient)
        @test !haskey(Bifti.to_dict(parse_phantom(phantom_json())), "patient")
        positioned = parse_phantom(phantom_json("\"patient\": {\"position\": \"HFDR\"},"))
        @test positioned.patient == Patient(Bifti.HFDR)
        @test scanner_matrix(positioned) == scanner_matrix(Bifti.HFDR)
        @test round_trip(positioned).patient == positioned.patient
    end

    @testset "tissue properties and defaults" begin
        tissue = parse_phantom(phantom_json("", """
            "T2": "maps.nii[2]", "dB0": {"file": "b0.nii.gz[0]", "func": "x - 420"},
            "B1+": [0.9, "b1.nii.gz[1]"]""")).tissues["gm"]
        @test tissue.density == NiftiRef("x.nii.gz", 0)
        @test tissue.T1 === Inf && tissue.T2dash === Inf
        @test tissue.T2 == NiftiRef("maps.nii", 2)
        @test tissue.dB0 == NiftiMapping(NiftiRef("b0.nii.gz", 0), "x - 420")
        @test tissue.ADC === 0.0
        @test tissue.B1_tx == [0.9, NiftiRef("b1.nii.gz", 1)]
        @test tissue.B1_rx == [1.0]
        # A NIfTI reference must name a sub-volume.
        @test_throws ArgumentError parse(NiftiRef, "x.nii.gz")
    end

    @testset "every example phantom round-trips" begin
        for name in ("shapes", "shapes_resliced", "shapes_downsampled", "subj42-3T")
            phantom = read_bifti(joinpath(DATA, "$name.json"))
            @test Bifti.to_dict(round_trip(phantom)) == Bifti.to_dict(phantom)
        end
    end

    @testset "unknown fields are kept, not rejected" begin
        json = phantom_json("\"from_the_future\": {\"a\": 1},", "\"T1\": 1.5, \"T22\": 0.1")
        phantom = @test_logs (:warn, r"from_the_future") (:warn, r"T22") parse_phantom(json)
        @test haskey(phantom.unknown, "from_the_future")
        @test haskey(phantom.tissues["gm"].unknown, "T22")
        # Saving must not silently drop what was not understood.
        reread = @test_logs (:warn, r"from_the_future") (:warn, r"T22") round_trip(phantom)
        @test reread.unknown == phantom.unknown
        @test reread.tissues["gm"].unknown == phantom.tissues["gm"].unknown
    end

    @testset "unsupported files are rejected" begin
        @test_throws ArgumentError parse_phantom(replace(phantom_json(), "bifti-phantom-v1" => "bifti-phantom-v2"))
        @test_throws ArgumentError parse_phantom(replace(phantom_json(), "\"Hz\"" => "\"rad/s\""))
    end

    @testset "referenced NIfTI files" begin
        @test nifti_files(read_bifti(joinpath(DATA, "subj42-3T.json"))) ==
              ["subj42.nii.gz", "subj42_dB0.nii.gz", "subj42_B1+.nii.gz"]
    end
end
