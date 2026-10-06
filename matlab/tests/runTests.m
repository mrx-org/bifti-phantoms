function runTests()
%RUNTESTS Run the bifti test suite; errors if any test fails.
%   Plain functions, so the same suite runs in MATLAB and GNU Octave:
%       cd matlab; addpath(pwd); addpath(fullfile(pwd, 'tests')); runTests
%   The tests load the example phantoms shared with the Python, Rust and Julia
%   packages from python/bifti/examples/data.
    tests = {
        @patientPositionsAreProperRotations
        @patientIsOptionalAndRoundTrips
        @tissuePropertiesAndDefaults
        @everyExamplePhantomRoundTrips
        @jsonWritesShortestNumbersAndNull
        @unknownFieldsAreKeptNotRejected
        @unsupportedFilesAreRejected
        @referencedNiftiFiles
        @funcFollowsTheSpecGrammar
        @funcRejectsAnythingElse
        @downsamplingIsAnExactBlockMean
        @t1IsNotDilutedAtTheFovEdge
        @emptyDensityFootprintFallsBackToPlainMean
        @upsamplingMatchesTrilinearInterpolation
        @identicalGridsAreAnExactIdentity
        @obliqueGridsUseQuadrature
        @complexDataAveragesInTheComplexPlane
        @voxelOnlyTouchingTheSourceGetsNoWeight
        @nativePhantomKeepsTheDensityGrid
        @resliceToBringsEveryMapOntoTheTarget
        @resamplingNeverInventsValuesOutsideTheSourceRange
        @downsamplingConservesTheTissueAmount
        @downsamplingAveragesInsteadOfPointSampling
        @multiChannelB1AndFuncOnAReslicedGrid
        @scannerAffineFollowsThePatientPosition
        @loadPhantomAcceptsStringsAndAParsedConfig
        @invalidReferencesFailLoudly
        @catalogLabelsResolveInTheRegistry
        @registryPhantomDownloadsAndCaches
    };
    failures = {};
    for k = 1:numel(tests)
        name = func2str(tests{k});
        try
            tests{k}();
            fprintf('  pass  %s\n', name);
        catch err
            fprintf('  FAIL  %s\n        %s\n', name, err.message);
            failures{end + 1} = name; %#ok<AGROW>
        end
    end
    fprintf('%d of %d tests passed\n', numel(tests) - numel(failures), numel(tests));
    if ~isempty(failures)
        error('bifti:testsFailed', '%d test(s) failed: %s', numel(failures), strjoin(failures, ', '));
    end
end

% ---------------------------------------------------------------------------
% Helpers
% ---------------------------------------------------------------------------

function path = dataPath(name)
    path = fullfile(fileparts(fileparts(fileparts(mfilename('fullpath')))), ...
        'python', 'bifti', 'examples', 'data', name);
end

function check(condition, varargin)
    if ~all(condition(:))
        error('bifti:test', varargin{:});
    end
end

function checkClose(actual, expected, tolerance, what)
    err = max(abs(actual(:) - expected(:)));
    check(isequal(size(actual), size(expected)) && err <= tolerance, ...
        '%s: max deviation %g exceeds %g', what, err, tolerance);
end

function checkError(f, id)
    try
        f();
    catch err
        check(strcmp(err.identifier, id), 'expected error %s, got %s (%s)', id, err.identifier, err.message);
        return
    end
    error('bifti:test', 'expected error %s, but nothing was thrown', id);
end

function config = parseQuietly(json)
    state = warning('off', 'bifti:unknownField');
    restore = onCleanup(@() warning(state));
    config = bifti.internal.parseConfig(bifti.internal.parseJson(json));
end

function json = phantomJson(extra, tissue)
    if nargin < 2
        tissue = '"T1": 1.5';
    end
    json = ['{"$schema": "bifti-phantom-v1.schema.json", "units": {"gyro": "MHz/T", "B0": "T", ' ...
        '"T1": "s", "T2": "s", "T2''": "s", "ADC": "10^-3 mm^2/s", "dB0": "Hz", "B1+": "rel", ' ...
        '"B1-": "rel"}, "system": {"gyro": 42.5764, "B0": 3.0}, ' extra ...
        '"tissues": {"gm": {"density": "x.nii.gz[0]", ' tissue '}}}'];
end

function config = roundTrip(config)
    path = [tempname '.json'];
    cleanup = onCleanup(@() delete(path));
    bifti.writePhantom(path, config);
    state = warning('off', 'bifti:unknownField');
    restore = onCleanup(@() warning(state));
    config = bifti.readPhantom(path);
end

function t = tissue(phantom, name)
    t = phantom.tissues(strcmp({phantom.tissues.name}, name));
end

function affine = gridAffine(spacing, origin)
    if nargin < 2
        origin = [0 0 0];
    end
    affine = [diag(spacing), origin(:)];
end

% With a uniform density the weighting cancels: a plain footprint average.
function y = uniformWeighted(r, x)
    weight = ones(size(x));
    weightSum = bifti.internal.contract(r, weight);
    y = bifti.internal.contract(r, x .* weight) ./ weightSum;
    y(weightSum == 0) = 0;  % outside the source: no taps, nothing to average
end

function s = shape3(x)
    s = [size(x, 1), size(x, 2), size(x, 3)];
end

% ---------------------------------------------------------------------------
% Phantom config
% ---------------------------------------------------------------------------

function patientPositionsAreProperRotations()
    % FFS is the identity, and a phantom without a position is not transformed.
    check(isequal(bifti.scannerMatrix('FFS'), eye(3)) && isequal(bifti.scannerMatrix(''), eye(3)), 'FFS must be the identity');
    % Every position is a proper rotation, and head first maps superior into the bore (-Z).
    for code = {'FFS', 'FFP', 'FFDR', 'FFDL', 'HFS', 'HFP', 'HFDR', 'HFDL'}
        P = bifti.scannerMatrix(code{1});
        check(isequal(P * P', eye(3)) && abs(det(P) - 1) < eps, '%s is not a proper rotation', code{1});
        check(P(3, 3) == 1 - 2 * strncmp(code{1}, 'HF', 2), '%s maps S onto the wrong end', code{1});
    end
    % HFS is FFS turned by 180 degrees about the vertical axis.
    check(isequal(bifti.scannerMatrix('HFS'), diag([-1 1 -1])), 'HFS must be diag(-1, 1, -1)');
    % Codes are case-sensitive, and non-MR DICOM codes are not supported.
    checkError(@() bifti.scannerMatrix('hfs'), 'bifti:patientPosition');
    checkError(@() bifti.scannerMatrix('SITTING'), 'bifti:patientPosition');
end

function patientIsOptionalAndRoundTrips()
    check(isempty(parseQuietly(phantomJson('')).patient), 'patient must default to empty (FFS)');
    positioned = parseQuietly(phantomJson('"patient": {"position": "HFDR"},'));
    check(strcmp(positioned.patient, 'HFDR'), 'patient position not parsed');
    check(isequal(bifti.scannerMatrix(positioned), bifti.scannerMatrix('HFDR')), 'scanner matrix of a config');
    check(strcmp(roundTrip(positioned).patient, 'HFDR'), 'patient position lost on round trip');
end

function tissuePropertiesAndDefaults()
    t = parseQuietly(phantomJson('', ['"T2": "maps.nii[2]", "dB0": {"file": "b0.nii.gz[0]", ' ...
        '"func": "x - 420"}, "B1+": [0.9, "b1.nii.gz[1]"]'])).tissues;
    check(isequal(t.density, struct('file', 'x.nii.gz', 'index', 0)), 'density reference');
    check(isequal(t.T2, struct('file', 'maps.nii', 'index', 2)), 'NIfTI reference');
    check(isequal(t.dB0, struct('file', 'b0.nii.gz', 'index', 0, 'func', 'x - 420')), 'transformed reference');
    % Omitted properties take the defaults of JSON.md.
    check(t.T1 == Inf && t.T2dash == Inf && t.ADC == 0 && isequal(t.B1_rx, {1}), 'defaults');
    check(t.B1_tx{1} == 0.9 && isequal(t.B1_tx{2}, struct('file', 'b1.nii.gz', 'index', 1)), 'B1+ channels');
    % A NIfTI reference must name a sub-volume.
    checkError(@() parseQuietly(phantomJson('', '"T2": "maps.nii"')), 'bifti:reference');
end

function everyExamplePhantomRoundTrips()
    for name = {'shapes', 'shapes_resliced', 'shapes_downsampled', 'subj42-3T'}
        config = bifti.readPhantom(dataPath([name{1} '.json']));
        check(isequal(roundTrip(config), config), '%s does not round-trip', name{1});
    end
end

function jsonWritesShortestNumbersAndNull()
    check(strcmp(bifti.internal.writeJson(0.1), '0.1'), '0.1 written as %s', bifti.internal.writeJson(0.1));
    check(strcmp(bifti.internal.writeJson(42.576), '42.576'), '42.576 written as %s', bifti.internal.writeJson(42.576));
    check(str2double(bifti.internal.writeJson(1/3)) == 1/3, '1/3 does not round-trip');
    check(strcmp(bifti.internal.writeJson([]), 'null'), 'null');
    check(strcmp(bifti.internal.writeJson({}), '[]'), 'empty array');
    checkError(@() bifti.internal.parseJson('{"a": "b'), 'bifti:json');
end

function unknownFieldsAreKeptNotRejected()
    json = phantomJson('"from_the_future": {"a": 1, "b": null, "c": []},', '"T1": 1.5, "T22": 0.1');
    lastwarn('');
    config = bifti.internal.parseConfig(bifti.internal.parseJson(json));
    [message, id] = lastwarn();
    check(strcmp(id, 'bifti:unknownField') && ~isempty(strfind(message, 'T22')), 'unknown field not warned about');
    check(isequal(config.unknown.keys, {'from_the_future'}), 'unknown phantom field not kept');
    check(isequal(config.tissues.unknown.keys, {'T22'}), 'unknown tissue field not kept');
    % Saving must not silently drop what was not understood.
    reread = roundTrip(config);
    check(isequal(reread.unknown, config.unknown) && isequal(reread.tissues.unknown, config.tissues.unknown), ...
        'unknown fields lost on round trip');
end

function unsupportedFilesAreRejected()
    checkError(@() parseQuietly(strrep(phantomJson(''), 'phantom-v1', 'phantom-v2')), 'bifti:schema');
    checkError(@() parseQuietly(strrep(phantomJson(''), '"Hz"', '"rad/s"')), 'bifti:units');
end

function referencedNiftiFiles()
    files = bifti.niftiFiles(bifti.readPhantom(dataPath('subj42-3T.json')));
    check(isequal(files, {'subj42.nii.gz', 'subj42_dB0.nii.gz', 'subj42_B1+.nii.gz'}), 'referenced files');
end

% ---------------------------------------------------------------------------
% func expressions
% ---------------------------------------------------------------------------

function funcFollowsTheSpecGrammar()
    x = [1; 2; 3; 6];
    % Numbers in every notation, + - * /, parentheses and the x* variables.
    check(isequal(bifti.internal.applyFunc('x - 420', x), x - 420), 'subtraction');
    check(isequal(bifti.internal.applyFunc('x * 0.5 + 10', x), x * 0.5 + 10), 'scale and offset');
    check(isequal(bifti.internal.applyFunc('(x - x_min) / (x_max - x_min)', x), (x - 1) / 5), 'min/max');
    checkClose(bifti.internal.applyFunc('(x - x_mean) / x_std', x), (x - 3) / sqrt(3.5), 1e-12, 'mean/std');
    check(isequal(bifti.internal.applyFunc('.5 * x + 1e-3 - -1.5', x), 0.5 * x + 1e-3 + 1.5), 'number notation');
    % Operators associate left, and * / bind tighter than + -.
    check(isequal(bifti.internal.applyFunc('x - 1 - 1 + 2 * 3 / 2', x), x + 1), 'precedence');
    % An expression without x still yields a map; complex maps stay complex.
    check(isequal(bifti.internal.applyFunc('2', x), 2 * ones(4, 1)), 'constant map');
    check(isequal(bifti.internal.applyFunc('x * 2', 1 + 1i), 2 + 2i), 'complex map');
end

function funcRejectsAnythingElse()
    % Nothing outside the grammar is evaluated.
    for func = {'x ^ 2', '2x', 'sin(x)', 'y', 'x +', '(x', 'x)', 'system(''ls'')', 'x; x', 'x + .', '.'}
        checkError(@() bifti.internal.applyFunc(func{1}, 1), 'bifti:func');
    end
end

% ---------------------------------------------------------------------------
% Resampling
% ---------------------------------------------------------------------------

function downsamplingIsAnExactBlockMean()
    for setup = [4 2; 8 4]'
        [n, factor] = deal(setup(1), setup(2));
        x = reshape(mod(0:n^3 - 1, 7), n, n, n);
        shift = (factor - 1) / 2;
        r = bifti.internal.resampler(gridAffine([1 1 1]), [n n n], ...
            gridAffine([factor factor factor], [shift shift shift]), [n n n] / factor);
        check(strcmp(r.kind, 'separable'), 'an axis-aligned grid must separate');
        y = uniformWeighted(r, x);
        block = squeeze(mean(mean(mean(reshape(x, factor, n / factor, factor, n / factor, factor, n / factor), 1), 3), 5));
        checkClose(y, block, 1e-12, sprintf('%dx block mean', factor));
    end
end

function t1IsNotDilutedAtTheFovEdge()
    % A voxel straddling the FOV edge must not average T1 against the zeros
    % outside it, while density does fall off there.
    density = ones(8, 8, 8);
    r = bifti.internal.resampler(gridAffine([1 1 1]), [8 8 8], gridAffine([4 4 4], [-2 -2 -2]), [4 4 4]);
    weight = density + 1e-6;
    t1 = bifti.internal.contract(r, 1.5 * weight) ./ bifti.internal.contract(r, weight);
    d = bifti.internal.contract(r, density);
    check(all(abs(t1(d > 1e-6) - 1.5) < 1e-4), 'T1 diluted at the FOV edge');
    check(any(d(:) > 0.99) && any(d(:) > 0 & d(:) < 0.9), 'density must fall off at the FOV edge');
end

function emptyDensityFootprintFallsBackToPlainMean()
    r = bifti.internal.resampler(gridAffine([1 1 1]), [4 4 4], gridAffine([2 2 2], [0.5 0.5 0.5]), [2 2 2]);
    weight = zeros(4, 4, 4) + 1;  % density 0 everywhere, regularised to 1
    y = bifti.internal.contract(r, 3.25 * weight) ./ bifti.internal.contract(r, weight);
    checkClose(y, 3.25 * ones(2, 2, 2), 1e-12, 'empty footprint');
end

function upsamplingMatchesTrilinearInterpolation()
    % Axes that are not downsampled keep plain trilinear interpolation;
    % interpn is an independent reference.
    x = reshape(sin(0:63), 4, 4, 4);
    y = uniformWeighted(bifti.internal.resampler(gridAffine([2 2 2]), [4 4 4], gridAffine([1 1 1]), [7 7 7]), x);
    c = (0:6) / 2;
    [q1, q2, q3] = ndgrid(c, c, c);
    expected = interpn(0:3, 0:3, 0:3, x, q1, q2, q3, 'linear', 0);
    checkClose(y, expected, 1e-12, 'upsampling');
end

function identicalGridsAreAnExactIdentity()
    affine = gridAffine([1.5 2 3], [-10 4 0.5]);
    x = reshape((0:59) * 0.37, 3, 4, 5);
    r = bifti.internal.resampler(affine, [3 4 5], affine, [3 4 5]);
    check(strcmp(r.kind, 'identity') && isequal(bifti.internal.contract(r, x), x), 'identity');
end

function obliqueGridsUseQuadrature()
    % A rotated grid cannot separate; a constant survives wherever it is fully covered.
    [c, s] = deal(0.8, 0.6);
    r = bifti.internal.resampler(gridAffine([1 1 1]), [8 8 8], [2*c -2*s 0 1; 2*s 2*c 0 1; 0 0 2 0.5], [4 4 4]);
    check(strcmp(r.kind, 'oblique'), 'a rotated grid must take the quadrature path');
    y = uniformWeighted(r, 2.5 * ones(8, 8, 8));
    check(any(abs(y(:) - 2.5) < 1e-9) && all(y(:) <= 2.5 + 1e-9), 'constant field not preserved');
end

function complexDataAveragesInTheComplexPlane()
    % B1 maps average as complex numbers: opposite phases cancel.
    x = reshape(repmat([1 + 0.5i, -1 - 0.5i], 1, 32), 4, 4, 4);
    y = uniformWeighted(bifti.internal.resampler(gridAffine([1 1 1]), [4 4 4], ...
        gridAffine([2 2 2], [0.5 0.5 0.5]), [2 2 2]), x);
    check(~isreal(x) && all(abs(y(:)) < 1e-12), 'complex values must cancel');
end

function voxelOnlyTouchingTheSourceGetsNoWeight()
    % Inverting the affine is inexact; a voxel that only touches the source must
    % still get zero weight. This is the geometry of shapes_downsampled.
    r = bifti.internal.resampler([3 0 0 -60; 0 3 0 -48; 0 0 5 -10], [40 32 4], ...
        [9 0 0 -66; 0 9 0 -51; 0 0 10 -12.5], [16 12 3]);
    weightSum = bifti.internal.contract(r, ones(40, 32, 4));
    check(all(all(all(weightSum([1 16], :, :) == 0))), 'touching voxels got weight');
end

% ---------------------------------------------------------------------------
% Loading
% ---------------------------------------------------------------------------

function nativePhantomKeepsTheDensityGrid()
    p = bifti.loadPhantom(dataPath('shapes.json'));
    for t = p.tissues
        check(isequal(shape3(t.density), [40 32 4]) && isequal(shape3(t.T1), [40 32 4]) && ...
            isequal(shape3(t.B1_tx{1}), [40 32 4]), '%s is not on the density grid', t.name);
    end
    % Scalars are expanded, defaults filled in and func applied voxel-wise.
    check(all(tissue(p, 'ring').T1(:) == 0.6) && all(tissue(p, 'background').T2dash(:) == Inf), 'scalars');
    checkClose(tissue(p, 'ring').dB0, tissue(p, 'disk').dB0 * 0.5 + 10, 1e-12, 'func mapping');
    check(numel(tissue(p, 'disk').B1_tx) == 2 && isequal({p.tissues.name}, {'disk', 'ring', 'background'}), ...
        'channels and tissue order');
end

function resliceToBringsEveryMapOntoTheTarget()
    targets = {'shapes_resliced', [60 48 4]; 'shapes_downsampled', [16 12 3]};
    for k = 1:size(targets, 1)
        p = bifti.loadPhantom(dataPath([targets{k, 1} '.json']));
        for t = p.tissues
            maps = [{t.density, t.T1, t.dB0}, t.B1_tx];
            check(all(cellfun(@(m) isequal(shape3(m), targets{k, 2}), maps)), '%s: wrong grid', targets{k, 1});
            check(all(isfinite(t.density(:))) && isequal(t.affine, p.config.resliceTo.affine), '%s', targets{k, 1});
        end
    end
end

function resamplingNeverInventsValuesOutsideTheSourceRange()
    % A density-weighted average is a convex combination of source values.
    native = bifti.loadPhantom(dataPath('shapes.json'));
    for name = {'shapes_resliced', 'shapes_downsampled'}
        p = bifti.loadPhantom(dataPath([name{1} '.json']));
        for t = p.tissues
            for property = {'T1', 'T2', 'dB0'}
                source = tissue(native, t.name).(property{1});
                check(min(t.(property{1})(:)) >= min(source(:)) - 1e-6 && ...
                    max(t.(property{1})(:)) <= max(source(:)) + 1e-6, '%s.%s leaves the source range', t.name, property{1});
            end
        end
    end
end

function downsamplingConservesTheTissueAmount()
    % Density is extensive: its integral over the FOV is invariant.
    native = bifti.loadPhantom(dataPath('shapes.json'));
    coarse = bifti.loadPhantom(dataPath('shapes_downsampled.json'));
    volume = @(t) abs(prod(diag(t.affine(:, 1:3))));
    for t = coarse.tissues
        fine = tissue(native, t.name);
        total = sum(fine.density(:)) * volume(fine);
        check(abs(sum(t.density(:)) * volume(t) - total) <= 0.02 * total, '%s: tissue amount not conserved', t.name);
    end
end

function downsamplingAveragesInsteadOfPointSampling()
    % Uniform background plus independent noise; each interior output voxel
    % averages 3x3x2 = 18 source voxels, so the noise drops by about sqrt(18).
    fine = tissue(bifti.loadPhantom(dataPath('shapes.json')), 'background').density;
    coarse = tissue(bifti.loadPhantom(dataPath('shapes_downsampled.json')), 'background').density;
    interior = coarse(3:end-2, 3:end-2, 2:end-1);
    check(abs(mean(interior(:)) - mean(fine(:))) <= 0.02 * mean(fine(:)), 'mean changed');
    expected = std(fine(:), 1) / sqrt(18);
    check(abs(std(interior(:), 1) - expected) <= 0.25 * expected, 'noise not averaged down');
end

function multiChannelB1AndFuncOnAReslicedGrid()
    p = bifti.loadPhantom(dataPath('subj42-3T.json'));
    for t = p.tissues
        check(isequal(shape3(t.density), [100 100 1]) && all(isfinite(t.dB0(:))), '%s', t.name);
        check(all(cellfun(@(c) isequal(shape3(c), [100 100 1]), t.B1_tx)), '%s B1+ grid', t.name);
    end
    check(numel(tissue(p, 'gm').B1_tx) == 8, 'gm must have 8 B1+ channels');
    % fat's dB0 is the shared dB0 map, density-weighted by fat, minus the 420 Hz
    % fat-water offset: shifted back, it stays within the source map's range.
    [source, ~] = bifti.internal.readNifti(dataPath('subj42_dB0.nii.gz'));
    shifted = tissue(p, 'fat').dB0(:) + 420;
    check(min(shifted) >= min(source(:)) - 1e-9 && max(shifted) <= max(source(:)) + 1e-9, 'fat dB0 offset');
end

function scannerAffineFollowsThePatientPosition()
    p = bifti.loadPhantom(dataPath('shapes.json'));
    affine = tissue(p, 'disk').affine;
    check(strcmp(p.config.patient, 'HFS'), 'shapes.json is HFS');
    check(isequal(bifti.scannerAffine(p, 'disk'), [diag([-1 1 -1]) * affine; 0 0 0 1]), 'HFS scanner affine');
    check(isequal(bifti.scannerAffine(affine, ''), [affine; 0 0 0 1]), 'no position leaves the affine');
end

function loadPhantomAcceptsStringsAndAParsedConfig()
    expected = bifti.loadPhantom(dataPath('shapes.json'));
    if exist('string', 'builtin') || exist('string', 'file')  % MATLAB string scalars
        check(isequal(bifti.loadPhantom(string(dataPath('shapes.json'))), expected), 'string path');
    end
    % Without a baseDir, relative NIfTI paths resolve against the current folder.
    previous = cd(fileparts(dataPath('shapes.json')));
    restore = onCleanup(@() cd(previous));
    check(isequal(bifti.loadPhantom(expected.config), expected), 'parsed config without baseDir');
end

function invalidReferencesFailLoudly()
    config = bifti.readPhantom(dataPath('shapes.json'));
    config.tissues = config.tissues(1);
    config.tissues.density.index = 3;  % the density file has 3 sub-volumes
    checkError(@() bifti.loadPhantom(config, fileparts(dataPath('shapes.json'))), 'bifti:reference');
end

% ---------------------------------------------------------------------------
% Registry
% ---------------------------------------------------------------------------

function catalogLabelsResolveInTheRegistry()
    root = fileparts(fileparts(fileparts(fileparts(fileparts(dataPath('x'))))));
    catalog = bifti.loadCatalog(fullfile(root, 'catalog.json'));
    registry = bifti.loadRegistry(fullfile(root, 'registry.json'));
    for collection = catalog.values
        files = bifti.flattenPhantoms(bifti.internal.jsonGet(registry, collection{1}));
        check(~isempty(files) && all(cellfun(@(f) numel(f) > 5 && strcmp(f(end-4:end), '.json'), files)), ...
            '%s lists no phantom JSONs', collection{1});
    end
    % Groups nest arbitrarily and are flattened depth-first.
    nested = bifti.internal.parseJson(['["a.json", {"group": "g", "phantoms": ["b.json", ' ...
        '{"group": "h", "phantoms": ["c.json"]}]}, "d.json"]']);
    check(isequal(bifti.flattenPhantoms(nested), {'a.json', 'b.json', 'c.json', 'd.json'}), 'nested groups');
end

function registryPhantomDownloadsAndCaches()
    % A local mirror of the Zenodo files API exercises the download path offline:
    % shapes.json only exists inside configs.tar, subj42-3T.json directly.
    mirror = tempname;
    record = fullfile(mirror, '123', 'files');
    cleanup = onCleanup(@() removeFolder(mirror));
    put = @(name, source) copyInto(fullfile(record, name), source);
    for name = {'subj42-3T.json', 'subj42.nii.gz', 'subj42_dB0.nii.gz', 'shapes_density.nii.gz', ...
            'shapes_dB0.nii.gz', 'shapes_B1.nii.gz'}
        put(name{1}, dataPath(name{1}));
    end
    put('subj42_B1+.nii.gz', dataPath('subj42_B1+.nii.gz'));
    tarDir = tempname;
    mkdir(tarDir);
    copyfile(dataPath('shapes.json'), tarDir);
    tar(fullfile(tarDir, 'configs.tar'), 'shapes.json', tarDir);
    put('configs.tar', fullfile(tarDir, 'configs.tar'));

    registry = bifti.internal.parseJson('{"x-demo-001": {"doi": "10.5281/zenodo.123"}}');
    cache = tempname;
    cleanupCache = onCleanup(@() removeFolder(cache));
    options = {'Registry', registry, 'CacheDir', cache, 'ZenodoApi', ['file://' mirror]};
    direct = bifti.loadRegistryPhantom('x-demo-001', 'subj42-3T.json', options{:});
    fromTar = bifti.loadRegistryPhantom('x-demo-001', 'shapes.json', options{:});
    check(numel(tissue(bifti.loadPhantom(direct), 'gm').B1_tx) == 8, 'direct download');
    check(isequal(shape3(tissue(bifti.loadPhantom(fromTar), 'disk').density), [40 32 4]), 'configs.tar fallback');
    % Cached files are reused: with the mirror gone, loading still succeeds.
    removeFolder(mirror);
    check(strcmp(bifti.loadRegistryPhantom('x-demo-001', 'shapes.json', options{:}), fromTar), 'cache');
    checkError(@() bifti.loadRegistryPhantom('x-demo-001', 'nope.json', options{:}), 'bifti:registry');
end

function removeFolder(folder)
    if exist(folder, 'dir')
        rmdir(folder, 's');
    end
end

function copyInto(dest, source)
    folder = fileparts(fullfile(dest, 'content'));
    if ~exist(folder, 'dir')
        mkdir(folder);
    end
    copyfile(source, fullfile(dest, 'content'));
end
