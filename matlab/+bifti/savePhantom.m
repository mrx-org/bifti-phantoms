function jsonPath = savePhantom(outDir, phantom, varargin)
%SAVEPHANTOM Save a loaded phantom as a self-contained folder.
%   jsonPath = bifti.savePhantom(outDir, phantom) writes the NIfTIs and the
%   phantom JSON outDir/phantom.json and returns its path;
%   bifti.savePhantom(outDir, phantom, jsonName) names the JSON differently.
%
%   Name-value options:
%     'ConstantThreshold'  also store a map as a constant when it is (nearly)
%                          one: when its RMS deviation from the real part of
%                          its mean - its standard deviation, for a real map -
%                          is at most the threshold, that mean replaces it. 0
%                          collapses exactly constant maps only; [] (the
%                          default) or a negative value keeps every map.
%                          density is always a map.
%
%   This writes what bifti.loadPhantom produced, so loading a phantom with a
%   reslice_to and saving it is how it is resliced on disk. The saved phantom
%   has no reslice_to and NO func mappings: every mapping was already applied
%   to the resampled data, and its reference becomes a plain one.
%
%   Layout: one gzip-compressed double (or complex double) NIfTI per property,
%   named after the JSON (phantom_density.nii.gz, phantom_T1.nii.gz,
%   phantom_B1plus_0.nii.gz, ... for phantom.json) so several phantoms can
%   share a folder, with the tissues stacked along the 4th dimension in order
%   of their sorted names. Properties that are constants in the phantom stay
%   constants in the JSON. The output is deterministic: the same phantom always
%   gives the same bytes.
%
%   WARNING: the result can be considerably larger than the input. Tissues that
%   referenced the same map (with or without different func) each get their own
%   copy of it, unless the copies are bit-identical, in which case they share
%   one volume. When reslicing, intensive properties are weighted by each
%   tissue's own density, so copies of one source map usually differ between
%   tissues. Data is stored as double (or complex double) whatever the type of
%   the source NIfTIs.
%
%   See also bifti.loadPhantom, bifti.writePhantom.
    jsonName = 'phantom.json';
    % An odd count means jsonName precedes the name-value pairs.
    if mod(numel(varargin), 2) == 1
        jsonName = char(varargin{1});
        varargin = varargin(2:end);
    end
    options = inputParser;
    options.addParameter('ConstantThreshold', []);
    options.parse(varargin{:});
    threshold = options.Results.ConstantThreshold;
    [~, prefix] = fileparts(jsonName);
    outDir = char(outDir);
    config = phantom.config;
    config.resliceTo = [];
    names = {config.tissues.name};
    if ~isequal(names, {phantom.tissues.name})
        error('bifti:save', 'phantom.tissues does not match phantom.config.tissues');
    end
    [~, order] = sort(names);
    config.tissues = config.tissues(order);
    tissues = phantom.tissues(order);
    if ~isfolder(outDir)
        mkdir(outDir);
    end

    properties = {'density', 'density', false; 'T1', 'T1', false; 'T2', 'T2', false; ...
        'T2dash', 'T2dash', false; 'ADC', 'ADC', false; 'dB0', 'dB0', false; ...
        'B1_tx', 'B1plus', true; 'B1_rx', 'B1minus', true};
    for p = 1:size(properties, 1)
        [field, stem, isChannels] = properties{p, :};
        nChannels = 1;
        if isChannels
            nChannels = max(arrayfun(@(t) numel(t.(field)), config.tissues));
        end
        for c = 1:nChannels
            % Constants stay in the JSON; only maps (reference structs) get a NIfTI.
            members = [];
            volumes = {};
            for k = 1:numel(config.tissues)
                property = config.tissues(k).(field);
                volume = tissues(k).(field);
                if isChannels
                    if c > numel(property)
                        continue
                    end
                    property = property{c};
                    volume = volume{c};
                end
                if ~isstruct(property)
                    continue
                end
                value = [];
                if ~strcmp(field, 'density')
                    value = asConstant(volume, threshold);
                end
                if isempty(value)
                    members(end + 1) = k; %#ok<AGROW>
                    volumes{end + 1} = volume; %#ok<AGROW>
                elseif isChannels
                    config.tissues(k).(field){c} = value;
                else
                    config.tissues(k).(field) = value;
                end
            end
            if isempty(members)
                continue
            end
            fileName = sprintf('%s_%s.nii.gz', prefix, stem);
            if isChannels
                fileName = sprintf('%s_%s_%d.nii.gz', prefix, stem, c - 1);
            end
            for i = 1:numel(members)
                if ~isequal(tissues(members(i)).affine, tissues(members(1)).affine)
                    error('bifti:save', '%s: tissues have different affines, cannot stack them into one NIfTI', fileName);
                end
            end
            indices = bifti.internal.writeNifti(fullfile(outDir, fileName), volumes, tissues(members(1)).affine);
            for i = 1:numel(members)
                ref = struct('file', fileName, 'index', indices(i));
                if isChannels
                    config.tissues(members(i)).(field){c} = ref;
                else
                    config.tissues(members(i)).(field) = ref;
                end
            end
        end
    end

    jsonPath = fullfile(outDir, jsonName);
    bifti.writePhantom(jsonPath, config);
end

function value = asConstant(volume, threshold)
%ASCONSTANT The real constant VOLUME can be stored as, or [] if it must stay a map.
%   That is the real part of its mean, if the RMS deviation from it is at most
%   THRESHOLD. An exactly constant real volume gives its value even at 0.
    value = [];
    if isempty(threshold) || threshold < 0 || isempty(volume)
        return
    end
    first = volume(1);
    if imag(first) == 0 && all(volume(:) == first)
        value = real(first);
        return
    end
    mu = real(mean(volume(:)));
    if sqrt(mean(abs(volume(:) - mu) .^ 2)) <= threshold
        value = mu;
    end
end
