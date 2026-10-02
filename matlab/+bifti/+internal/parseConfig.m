function config = parseConfig(json)
%PARSECONFIG Phantom config struct (see bifti.readPhantom) from parsed JSON.
    field = @bifti.internal.jsonGet;
    phantomFields = {'$schema', 'units', 'system', 'patient', 'reslice_to', 'tissues', 'file_type'};
    tissueFields = {'density', 'T1', 'T2', 'T2''', 'ADC', 'dB0', 'B1+', 'B1-'};

    schema = field(json, '$schema', field(json, 'file_type', ''));
    if isempty(regexp(schema, '(nifti|bifti)[-_]phantom[-_]v1(\.[^/]*)?$', 'once'))
        error('bifti:schema', 'Unsupported $schema: "%s"', schema);
    end
    % Only the default units are supported, as in the other implementations.
    units = field(json, 'units');
    default = bifti.internal.defaultUnits();
    if numel(units.keys) ~= numel(default.keys) || ...
            ~all(cellfun(@(k, v) isequal(field(units, k, []), v), default.keys, default.values))
        error('bifti:units', 'Only the default units are supported (JSON.md)');
    end
    warnUnknown('the phantom', json, phantomFields);

    system = field(json, 'system');
    warnUnknown('system', system, {'gyro', 'B0'});
    config.schema = schema;
    config.system = struct('gyro', field(system, 'gyro'), 'B0', field(system, 'B0'));

    config.patient = '';
    patient = field(json, 'patient', []);
    if ~isempty(patient)
        warnUnknown('patient', patient, {'position'});
        config.patient = field(patient, 'position');
        bifti.scannerMatrix(config.patient);  % validates the code
    end

    config.resliceTo = [];
    reslice = field(json, 'reslice_to', []);
    if ~isempty(reslice)
        warnUnknown('reslice_to', reslice, {'affine', 'resolution'});
        rows = field(reslice, 'affine');
        config.resliceTo = struct('affine', cell2mat(cellfun(@(r) cell2mat(r), rows(:), 'UniformOutput', false)), ...
            'resolution', cell2mat(field(reslice, 'resolution')));
    end

    tissues = field(json, 'tissues');
    config.tissues = struct('name', {}, 'density', {}, 'T1', {}, 'T2', {}, 'T2dash', {}, ...
        'ADC', {}, 'dB0', {}, 'B1_tx', {}, 'B1_rx', {}, 'unknown', {});
    for k = 1:numel(tissues.keys)
        name = tissues.keys{k};
        t = tissues.values{k};
        warnUnknown(sprintf('tissue "%s"', name), t, tissueFields);
        config.tissues(k).name = name;
        config.tissues(k).density = parseRef(field(t, 'density'));
        config.tissues(k).T1 = parseProperty(field(t, 'T1', Inf));
        config.tissues(k).T2 = parseProperty(field(t, 'T2', Inf));
        config.tissues(k).T2dash = parseProperty(field(t, 'T2''', Inf));
        config.tissues(k).ADC = parseProperty(field(t, 'ADC', 0));
        config.tissues(k).dB0 = parseProperty(field(t, 'dB0', 0));
        config.tissues(k).B1_tx = cellfun(@parseProperty, field(t, 'B1+', {1}), 'UniformOutput', false);
        config.tissues(k).B1_rx = cellfun(@parseProperty, field(t, 'B1-', {1}), 'UniformOutput', false);
        config.tissues(k).unknown = unknownFields(t, tissueFields);
    end
    config.unknown = unknownFields(json, phantomFields);
end

function property = parseProperty(value)
    if isnumeric(value)
        property = value;
    elseif ischar(value)
        property = parseRef(value);
    else
        warnUnknown('a transformed reference', value, {'file', 'func'});
        property = parseRef(bifti.internal.jsonGet(value, 'file'));
        property.func = bifti.internal.jsonGet(value, 'func');
    end
end

function ref = parseRef(value)
    tokens = regexp(value, '^(.+?)\[(\d+)\]$', 'tokens', 'once');
    if isempty(tokens)
        error('bifti:reference', 'Invalid NIfTI reference "%s", expected "<file>[<index>]"', value);
    end
    ref = struct('file', tokens{1}, 'index', str2double(tokens{2}));
end

% The format is additively extensible (SPEC.md): unknown fields are ignored,
% but warned about, since the schema can no longer catch typos.
function warnUnknown(where, object, known)
    for key = object.keys(~ismember(object.keys, known))
        warning('bifti:unknownField', 'Ignoring unknown field "%s" in %s', key{1}, where);
    end
end

function unknown = unknownFields(object, known)
    keep = ~ismember(object.keys, known);
    unknown = struct('keys', {object.keys(keep)}, 'values', {object.values(keep)});
end
