%% simple_fp_preprocess.m
%  Minimal single-channel preprocessing for GRAB-ACh3.0 fiber photometry
%  ---------------------------------------------------------------------
%  Steps:
%    1. skip cable-attachment artefact (first N seconds)
%    2. 4th-order 6 Hz zero-phase Butterworth low-pass
%    3. down-sample to 20 Hz
%    4. robust bi-exponential photobleach fit → F0(t)
%    5. ΔF/F (%)  =  (F − F0) / F0
%    6. derivative-MAD spike culling
%    7. quick plots + CSV export
%
%  INPUT (must exist in workspace):
%         datagreen : raw 465-nm fluorescence vector
%         ts        : time-stamp vector (s), same length as datagreen
%
%  OUTPUT file:
%         dFF_trace.csv  —  two columns: time_s , dFF_percent
%
%  Author: Douglas Albert      Date: <today>
% ----------------------------------------------------------------------

%% ---------- user-tunable parameters -----------------------------------
Fs_raw   = 1017;   % raw sample rate (Hz)
skip_s   = 10;     % seconds to discard at start
Fs_out   = 20;     % target sample rate (Hz)
lp_cut   = 6;      % low-pass cut-off (Hz)
spk_k    = 8;      % MAD multiplier for spike detector
smooth_s = 1;      % seconds for display smoothing (0 = off)
% ----------------------------------------------------------------------

%% 1.  Skip initial artefact
skip_idx = round(skip_s * Fs_raw);
F465 = datagreen(skip_idx + 1:end);
t    = ts(skip_idx + 1:end);

%% 2.  Low-pass filter (4th-order Butterworth, zero-phase)
[b,a] = butter(4, lp_cut / (Fs_raw/2), 'low');
F465  = filtfilt(b,a,F465);

%% 3.  Down-sample
decim = round(Fs_raw / Fs_out);
F465  = F465(1:decim:end);
t     = t(1:decim:end);
Fs    = Fs_out;                       % effective Fs after decimation

%% 4.  Robust bi-exponential photobleach fit
tt   = (0:numel(F465)-1)'/Fs;         % column time base
F465 = F465(:);                       % ensure column vector

% --- initial parameter guesses (5 params: a b t1 t2 c) -----------------
A1   = max(F465) - min(F465);    % fast decay amplitude
A2   = 0.3 * A1;                 % slow decay amplitude
tau1 = 5;                        % fast time-constant (s)
tau2 = 120;                      % slow time-constant (s)
C0   = min(F465);                % offset
startP = [A1, A2, tau1, tau2, C0];

mdl = fit(tt, F465, 'a*exp(-x/t1) + b*exp(-x/t2) + c', ...
          'Start',  startP, ...
          'Lower',  [0 0 0 0 -Inf], ...
          'Robust', 'Bisquare');

F0 = mdl(tt);                     % fitted baseline

%% 5.  ΔF/F (%)
dFF = (F465 - F0) ./ (F0 + eps) * 100;

%% 6.  Motion-spike removal (derivative MAD)
dFdt = [0; diff(dFF)];
thr   = spk_k * mad(dFdt,1);
bad   = abs(dFdt) > thr;
if any(bad)
    good = find(~bad);
    dFF(bad) = interp1(good, dFF(good), find(bad), 'linear', 'extrap');
end

%% 7.  (optional) smoothing for display only
if smooth_s > 0
    w        = round(smooth_s * Fs);
    dFF_disp = movmean(dFF, w);
else
    dFF_disp = dFF;
end

%% 8.  Quick plots
figure;                                % raw + baseline
plot(t, F465, 'Color', [0.2 0.7 1]); hold on;
plot(t, F0, 'k', 'LineWidth', 1.2);
xlabel('Time (s)'); ylabel('Fluorescence (a.u.)');
title('Raw 465-nm signal and fitted baseline'); legend('465 nm','F₀(t)');

figure;                                % ΔF/F
plot(t, dFF_disp, 'b');
xlabel('Time (s)'); ylabel('\DeltaF/F (%)');
title('Cleaned \DeltaF/F (%)');

%% 9.  Export CSV
writetable(table(t(:), dFF(:)), 'dFF_trace.csv', ...
           'VariableNames', {'time_s','dFF_percent'});
disp('Saved → dFF_trace.csv');