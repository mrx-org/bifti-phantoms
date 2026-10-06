function units = defaultUnits()
%DEFAULTUNITS The fixed units of BIfTI v1 (JSON.md); parsers never convert.
    units = bifti.internal.jsonObject('gyro', 'MHz/T', 'B0', 'T', 'T1', 's', 'T2', 's', ...
        'T2''', 's', 'ADC', '10^-3 mm^2/s', 'dB0', 'Hz', 'B1+', 'rel', 'B1-', 'rel');
end
