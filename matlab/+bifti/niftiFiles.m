function files = niftiFiles(config)
%NIFTIFILES Every distinct NIfTI file a phantom config references, in order of use.
%   See also bifti.readPhantom.
    files = {};
    for t = config.tissues(:)'
        properties = [{t.density, t.T1, t.T2, t.T2dash, t.ADC, t.dB0}, t.B1_tx, t.B1_rx];
        for p = properties
            if isstruct(p{1}) && ~any(strcmp(files, p{1}.file))
                files{end + 1} = p{1}.file; %#ok<AGROW>
            end
        end
    end
end
