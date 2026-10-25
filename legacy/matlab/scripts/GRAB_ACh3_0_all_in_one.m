% GRABBER v0.3.8 - GRAB-ACh3.0 Robust Baseline Extraction Routine
%
% This script provides a simple interface to run a comprehensive
% GRAB-ACh3.0 analysis pipeline with multiple, selectable state-of-the-art paradigms.
%
% Author: Douglas Albert
% Date: 2025-06-08 
% Version: 0.3.8
%
% DEPENDENCIES:
% 1. MATLAB Signal Processing Toolbox
% 2. MATLAB Statistics and Machine Learning Toolbox
% 3. TDTbin2mat.m (must be in the MATLAB path)

function GRABBER_0_3_8()
%% GRAB-ACh3.0 Fiber Photometry Analysis (GRABBER v0.3.8)
% INSTRUCTIONS:
% 1. Update the data path below to match your TDT data location.
% 2. Select a 'processing_paradigm'.
% 3. Configure the 'analysis_window' parameters below.
% 4. Run this script and check the generated figures and CSV files.

% =================================================================
%                    USER CONFIGURATION
% =================================================================

% --- Data Location ---
data_path = '/path/to/your/data'; % <<< UPDATE
experiment_date = 'YYYY-MM-DD'; % <<< UPDATE
subject_id = 'SUBJECT';
experiment_id = 'EXPERIMENT';

% --- Master Analysis Selection ---
params = struct(); 
params.processing_paradigm = 'subtraction'; % Options: 'standard', 'beads', 'subtraction', 'isosbestic'

% --- General & Shared Parameters ---
params.subject_id = subject_id; params.experiment_id = experiment_id; params.experiment_date = experiment_date; 
params.data_path = data_path; params.original_fs = 1017.3; params.target_fs = 19;

% --- Final Analysis Window Definition --- change this for each sample
params.analysis_window_start_seconds = 3;
params.analysis_window_end_seconds = 0;

% =================================================================
%             PARADIGM-SPECIFIC PARAMETERS
% =================================================================
shared_beads_params = {'beads_run_on','signal_465', 'beads_fc',0.0005, 'beads_d',2, 'beads_r',6, 'beads_lam0',0.5, 'beads_lam1',1.0, 'beads_lam2',4.0};

switch params.processing_paradigm
    case 'standard'
        fprintf('Loading STANDARD paradigm...\n');
        params.F0_estimation_method = 'biexponential_on_465'; params.biexp_robust_fitting = true;    
        params.use_beads_denoising = false; params = set_shared_params(params, shared_beads_params);
        params.use_irls = false;
        
    case 'beads'
        fprintf('Loading BEADS paradigm...\n');
        params.use_beads_denoising = true; params = set_shared_params(params, shared_beads_params);
        params.F0_estimation_method = 'beads_baseline';
        params.use_irls = false;

    case 'subtraction'
        fprintf('Loading SUBTRACTION paradigm...\n');
        params.use_beads_denoising = false; params = set_shared_params(params, shared_beads_params);
        params.F0_estimation_method = 'polynomial_on_corrected'; params.poly_fit_order = 2;             
        params.use_irls = false;

    case 'isosbestic'
        fprintf('Loading ISOSBESTIC paradigm...\n');
        params.use_beads_denoising = false; params = set_shared_params(params, shared_beads_params);
        params.F0_estimation_method = 'isosbestic';
        params.use_irls = true;
end
params.lowpass_cutoff = 6; params.filter_order = 4;
params.irls_tune = 4.685; params.irls_max_iter = 75;

% --- Shared Output Parameters ---
params.plot_steps = true; params.save_figures = true; params.figure_format = 'png';          
params.export_processed_data = true; params.export_summary_stats = true;    

% =================================================================
%                    MAIN ANALYSIS PIPELINE
% =================================================================
fprintf('\n=== GRABBER v0.3.8 Analysis ===\n');
fprintf('Paradigm: ''%s''\n', params.processing_paradigm);

try
    check_dependencies();
    
    [raw_data, fs_original] = load_tdt_data(params.data_path);
    if abs(fs_original - params.original_fs) > 1, params.original_fs = fs_original; end
    
    processed_data_long = preprocess_signals(raw_data, params);
    
    corrected_data_long = correct_motion_artifacts(processed_data_long, params);
    
    [F0_long, signal_for_dff_long] = estimate_f0_baseline(processed_data_long, corrected_data_long, params);
    
    [processed_data_final, F0_final, signal_for_dff_final] = ...
        trim_analysis_window(processed_data_long, F0_long, signal_for_dff_long, params);
    
    normalized_data = calculate_delta_f_over_f(signal_for_dff_final, F0_final, processed_data_final, params);
    
    if params.plot_steps
        fig = create_analysis_summary_plots(raw_data, processed_data_long, processed_data_final, normalized_data, F0_long, params);
        if params.save_figures
            export_figure(fig, params);
        end
    end
    
    if params.export_processed_data, export_analysis_results(normalized_data, params); end
    if params.export_summary_stats, generate_analysis_summary(normalized_data, params); end
    
    fprintf('\n=== Analysis completed successfully! ===\n');
catch ME, fprintf(2, '\nERROR in GRABBER pipeline: %s\n', ME.message); for i = 1:length(ME.stack), fprintf(2, '  In %s (line %d)\n', ME.stack(i).name, ME.stack(i).line); end, rethrow(ME); end
end

% =================================================================
% --- HELPER FUNCTIONS ---
% =================================================================

function params_out = set_shared_params(params_in, shared_params_cell), params_out = params_in; for i = 1:2:length(shared_params_cell), params_out.(shared_params_cell{i}) = shared_params_cell{i+1}; end, end
function check_dependencies(), fprintf('Step 0: Checking dependencies...\n'); if ~exist('TDTbin2mat', 'file'), error('GRABBER:DependencyNotFound', 'TDTbin2mat function not found.'); end, if ~license('test', 'Signal_Toolbox'), error('GRABBER:DependencyNotFound', 'Signal Processing Toolbox not found.'); end, if ~license('test', 'Statistics_Toolbox'), error('GRABBER:DependencyNotFound', 'Statistics and Machine Learning Toolbox not found.'); end, fprintf('  - All dependencies found.\n'); end
function [raw_data, fs] = load_tdt_data(data_path), fprintf('Step 1: Loading TDT data...\n'); data = TDTbin2mat(data_path); if ~isfield(data, 'streams') || ~isfield(data.streams, 'x465A') || ~isfield(data.streams, 'x405A'), error('GRABBER:DataStreamMissing', 'Required streams (x465A, x405A) not found.'); end, signal_465 = data.streams.x465A.data; control_405 = data.streams.x405A.data; fs = data.streams.x465A.fs; minLength = min(length(signal_465), length(control_405)); if length(signal_465) ~= length(control_405), warning('GRABBER:UnequalSignalLengths', 'Signals have different lengths. Truncating to shorter length (%d samples).', minLength); signal_465=signal_465(1:minLength); control_405=control_405(1:minLength); end, time_vector = (0:minLength-1) / fs; raw_data = struct('signal_465', signal_465, 'control_405', control_405, 'time', time_vector, 'fs', fs); end
function processed_data = preprocess_signals(raw_data, params), fprintf('Step 2: Preprocessing full signals...\n'); fs_orig = raw_data.fs; nyquist_freq = fs_orig / 2; if params.lowpass_cutoff >= nyquist_freq, error('GRABBER:InvalidFilterParams', 'Lowpass cutoff must be less than Nyquist frequency.'); end, [b,a] = butter(params.filter_order, params.lowpass_cutoff/nyquist_freq, 'low'); sig465_filt = filtfilt(b,a,double(raw_data.signal_465)); sig405_filt = filtfilt(b,a,double(raw_data.control_405)); fs_new = params.target_fs; [sig465_ds, time_ds] = resample(sig465_filt, raw_data.time, fs_new); sig405_ds = resample(sig405_filt, raw_data.time, fs_new); processed_data = struct('signal_465',sig465_ds(:),'control_405',sig405_ds(:),'time',time_ds(:),'fs',fs_new); end
function corrected_data = correct_motion_artifacts(processed_data, params), fprintf('Step 3: Performing motion correction on full signal...\n'); if params.use_irls, fitted_control = fit_irls_control(processed_data.control_405, processed_data.signal_465, params); else, fitted_control = fit_ols_control(processed_data.control_405, processed_data.signal_465); end, corrected_data = processed_data; corrected_data.fitted_control = fitted_control; corrected_data.signal_corrected = processed_data.signal_465 - fitted_control; end

function [F0_baseline, signal_for_dff] = estimate_f0_baseline(processed_data, corrected_data, params)
    fprintf('Step 4: Estimating F0 baseline on full signal...\n');
    signal_for_dff = processed_data.signal_465;
    switch params.F0_estimation_method
        case 'beads_baseline', if ~params.use_beads_denoising, error('BEADS selected, but use_beads_denoising is false.'); end; beads_results = apply_beads_denoising(processed_data, params); F0_baseline = beads_results.estimated_baseline_f;
        case 'polynomial_on_corrected', signal_for_dff = corrected_data.signal_corrected; F0_baseline = fit_polynomial(processed_data.time, signal_for_dff, params.poly_fit_order);
        case 'biexponential_on_465', F0_baseline = fit_biexponential_decay(processed_data.time, signal_for_dff, params.biexp_robust_fitting);
        case 'isosbestic', F0_baseline = corrected_data.fitted_control;
        otherwise, error('F0 estimation method not recognized: %s', params.F0_estimation_method);
    end
end
function beads_results = apply_beads_denoising(processed_data, params), fprintf('  - Applying BEADS algorithm...\n'); y = processed_data.(params.beads_run_on); mean_y = mean(y, 'omitnan'); y_centered = y - mean_y; [~, baseline_centered_f, ~] = beads(y_centered, params.beads_d, params.beads_fc, params.beads_r, params.beads_lam0, params.beads_lam1, params.beads_lam2); beads_results = struct('estimated_baseline_f', baseline_centered_f + mean_y); end

function [processed_final, F0_final, signal_for_dff_final] = trim_analysis_window(processed_long, F0_long, signal_for_dff_long, params)
    fprintf('Step 5: Trimming signals to final analysis window...\n');
    time_vector = processed_long.time;
    idx_start = find(time_vector >= params.analysis_window_start_seconds, 1, 'first');
    idx_end = find(time_vector <= (time_vector(end) - params.analysis_window_end_seconds), 1, 'last');
    if isempty(idx_start) || isempty(idx_end) || idx_start >= idx_end, error('GRABBER:InvalidAnalysisWindow', 'Analysis window is invalid or results in zero length. Adjust start/end seconds.'); end
    fprintf('  - Final analysis window: %.2f s to %.2f s.\n', time_vector(idx_start), time_vector(idx_end));
    
    processed_final = processed_long;
    processed_final.signal_465 = processed_long.signal_465(idx_start:idx_end);
    processed_final.control_405 = processed_long.control_405(idx_start:idx_end);
    processed_final.time = processed_long.time(idx_start:idx_end);
    processed_final.time = processed_final.time - processed_final.time(1);

    F0_final = F0_long(idx_start:idx_end);
    signal_for_dff_final = signal_for_dff_long(idx_start:idx_end);
end

function normalized_data = calculate_delta_f_over_f(signal_final, F0_final, processed_final, params)
    fprintf('Step 6: Calculating dF/F on final, trimmed data...\n');
    
    % The method of calculating dF/F depends critically on the paradigm.
    if strcmp(params.F0_estimation_method, 'polynomial_on_corrected')
        % For this special case, the 'signal' is the motion-corrected trace,
        % and F0 is its residual baseline. The dF is their difference.
        % The denominator F0 must be a measure of total fluorescence, NOT
        % the near-zero baseline of the corrected signal.
        dF = signal_final - F0_final;
        F0_for_division = mean(processed_final.signal_465); % Use mean of the original signal
        delta_f_over_f = dF / F0_for_division;
        fprintf('  - Using special dF/F calculation for subtraction paradigm.\n');
    else
        % For all other methods, the standard (F-F0)/F0 is used.
        F0_safe = F0_final; F0_safe(F0_safe <= 0) = 1e-9;
        delta_f_over_f = (signal_final - F0_safe) ./ F0_safe;
    end
    
    normalized_data = struct('signal_for_dff',signal_final, 'F0_used_for_dff',F0_final, 'delta_f_over_f',delta_f_over_f * 100, 'time',processed_final.time, 'fs',processed_final.fs, 'F0_method',params.F0_estimation_method);
end

% --- FITTING, PLOTTING & EXPORT FUNCTIONS ---
function fitted_control = fit_ols_control(control, signal), X=[ones(length(control),1), control(:)]; y=signal(:); beta=X\y; fitted_control=X*beta; end
function fitted_control = fit_irls_control(control, signal, params), X=[ones(length(control),1),control(:)]; y=signal(:); beta=X\y; for iter=1:params.irls_max_iter, res=y-(X*beta); mad_r=mad(res,1); if mad_r<1e-6, break; end, u=res./(mad_r*1.4826*params.irls_tune); w=(abs(u)<1).*((1-u.^2).^2); beta_new=(X.*w)\(y.*w); if norm(beta_new-beta)<1e-8, break; end; beta=beta_new; end, fitted_control=X*beta; end
function F0_poly = fit_polynomial(time, signal, order), [p,~,mu]=polyfit(time(:),signal(:),order); F0_poly=polyval(p,time(:),[],mu); end
function F0_biexp = fit_biexponential_decay(time,signal,is_robust), time_d=double(time(:)); signal_d=double(signal(:)); model=fittype('a*exp(-x/t1)+b*exp(-x/t2)+c','independent','x','dependent','y'); opts=fitoptions('Method','NonlinearLeastSquares','Display','off'); s_end=mean(signal_d(max(1,end-20):end)); s_std=std(signal_d); t_max=max(time_d); opts.StartPoint=[s_std,t_max/10,s_std/2,t_max/2,s_end]; if is_robust, opts.Robust='LAR'; end; try [fit_obj,~]=fit(time_d,signal_d,model,opts); F0_biexp=feval(fit_obj,time_d); catch ME, warning('Bi-exponential fit failed: %s. Using linear detrend.',ME.message); F0_biexp=detrend(signal_d,'linear')+mean(signal_d); end, end

function fig = create_analysis_summary_plots(raw_data, processed_long, processed_final, normalized_data, F0_long, params)
    fprintf('Step 7: Generating analysis summary plots...\n');
    fig = figure('Name','GRABBER Analysis Summary','Position',[100,100,1200,900],'Color','w');
    c465=[0,0.6,0.2]; c405=[0.6,0,0.8]; cF0=[1,0,0]; cLightGray=[0.85,0.85,0.85]; cLightRed=[1,0.7,0.7];
    
    subplot(4,1,1); plot(raw_data.time/60, raw_data.signal_465,'Color',c465); hold on; plot(raw_data.time/60, raw_data.control_405,'Color',c405); title('Raw Untrimmed Signals'); xlabel('Time (min)'); ylabel('Fluorescence (AU)'); legend('465nm','405nm','Location','best'); grid on; xlim tight;
    
    subplot(4,1,2); time_long_min = processed_long.time/60; time_final_min = processed_final.time/60 + params.analysis_window_start_seconds/60;
    plot(time_long_min, processed_long.signal_465, 'Color', cLightGray); hold on;
    plot(time_long_min, F0_long, '--', 'Color', cLightRed, 'LineWidth', 1);
    plot(time_final_min, processed_final.signal_465, 'k', 'LineWidth', 1.5);
    plot(time_final_min, normalized_data.F0_used_for_dff, '--r', 'LineWidth', 1.5);
    title('Filtered & Downsampled Signal (Analysis Window Highlighted)'); legend('Full Signal', 'Full F0 Fit', 'Analyzed Signal', 'Analyzed F0 Fit','Location','best');
    xlabel('Time (min)'); ylabel('Fluorescence (AU)'); grid on; xlim(time_long_min([1 end]));

    subplot(4,1,3); plot(normalized_data.time/60, normalized_data.signal_for_dff, 'Color', [0.5 0.5 0.5]); hold on;
    plot(normalized_data.time/60, normalized_data.F0_used_for_dff, 'Color', cF0, 'LineWidth', 1.5);
    title(['F0 Estimation on Final Data | Method: ' strrep(normalized_data.F0_method,'_',' ')]);
    xlabel('Time (min, relative to analysis window start)'); ylabel('Fluorescence (AU)'); legend('Signal for dF/F', 'F0 Baseline','Location','best'); grid on; xlim tight;

    subplot(4,1,4); plot(normalized_data.time/60, normalized_data.delta_f_over_f, 'Color', c465, 'LineWidth', 1.5); yline(0,'k--');
    title('Final dF/F Signal (from Analysis Window)'); xlabel('Time (min, relative to analysis window start)'); ylabel('dF/F (%)'); grid on; xlim tight;
    
    sgtitle(sprintf('GRABBER Analysis | Subject: %s | Paradigm: %s', params.subject_id, params.processing_paradigm),'FontWeight','bold');
end

function export_figure(fig_handle, params), output_dir='./GRAB_ACh3_0_Analysis_Figures'; if ~exist(output_dir,'dir'), mkdir(output_dir); end, base_filename=generate_base_filename(params); saveas(fig_handle,fullfile(output_dir,[base_filename '_SummaryPlot']),params.figure_format); fprintf('Saved summary figure to: %s\n',output_dir); end
function export_analysis_results(normalized_data, params), output_dir='./GRAB_ACh3_0_Analysis_Data_Exports'; if ~exist(output_dir,'dir'), mkdir(output_dir); end, base_filename=generate_base_filename(params); dt=table(normalized_data.time,normalized_data.delta_f_over_f,'VariableNames',{'Time_sec','DeltaF_over_F_Percent'}); writetable(dt,fullfile(output_dir,[base_filename '_Data.csv'])); p_names=fieldnames(params)'; p_values=struct2cell(params)'; pt=table(p_names',cellfun(@param2str,p_values,'UniformOutput',false)','VariableNames',{'Parameter','Value'}); writetable(pt,fullfile(output_dir,[base_filename '_Parameters.csv'])); fprintf('Exported data and parameters to: %s\n',output_dir); end
function base_filename = generate_base_filename(params), ts=datestr(now,'yyyymmdd_HHMMSS'); s_id=matlab.lang.makeValidName(params.subject_id); e_id=matlab.lang.makeValidName(params.experiment_id); paradigm_name=matlab.lang.makeValidName(params.processing_paradigm); base_filename=sprintf('%s_%s_%s_%s',s_id,e_id,paradigm_name,ts); end
function str_val = param2str(val), if ischar(val)||isstring(val),str_val=char(val); elseif isnumeric(val)&&isscalar(val),str_val=num2str(val); elseif isnumeric(val),str_val=mat2str(val); elseif islogical(val),if val,str_val='true';else,str_val='false';end; else,str_val=['[unsupported: ' class(val) ']']; end, end
function generate_analysis_summary(normalized_data, params), fprintf('\n=== Analysis Summary ===\n'); fprintf('Paradigm: ''%s'', F0 Method: ''%s''\n',params.processing_paradigm,normalized_data.F0_method); dff=normalized_data.delta_f_over_f; fprintf('dF/F Stats -> Mean: %.2f%%, Std: %.2f%%, Median: %.2f%%, Range: [%.2f%%, %.2f%%]\n',mean(dff,'omitnan'),std(dff,0,'omitnan'),median(dff,'omitnan'),min(dff),max(dff)); end

% --- BEADS ALGORITHM ---
function [x,f,cost]=beads(y,d,fc,r,lam0,lam1,lam2), Nit=30;pen='L1_v2';EPS0=1e-6;EPS1=1e-6; switch pen, case 'L1_v2',phi=@(x)abs(x)-EPS1*log(abs(x)+EPS1);wfun=@(x)1./(abs(x)+EPS1);otherwise,error('penalty must be L1_v1 or L1_v2');end, theta=@(x)sum(x(x>EPS0))-r*sum(x(x<-EPS0))+sum((1+r)/(4*EPS0)*x(abs(x)<=EPS0).^2+(1-r)/2*x(abs(x)<=EPS0)+EPS0*(1+r)/4); y=y(:);N=length(y);[A,B]=BAfilt(d,fc,N);H=@(x)B*(A\x);D1=spdiags([-ones(N-1,1) ones(N-1,1)],[0 1],N-1,N);D2=spdiags([ones(N-2,1) -2*ones(N-2,1) ones(N-2,1)],0:2,N-2,N);D=[D1;D2];BTB=B'*B;w=[lam1*ones(N-1,1);lam2*ones(N-2,1)];b_asym=(1-r)/2*ones(N,1);d_matrix=BTB*(A\y)-lam0*(A'*b_asym);gamma=ones(N,1);x=y;cost=zeros(1,Nit); for i=1:Nit, Lambda=spdiags(w.*wfun(D*x),0,2*N-3,2*N-3);k=abs(x)>EPS0;gamma(~k)=((1+r)/4)/abs(EPS0);gamma(k)=((1+r)/4)./abs(x(k));Gamma=spdiags(gamma,0,N,N);M=2*lam0*Gamma+D'*Lambda*D;x=A*((BTB+A'*M*A)\d_matrix);cost(i)=0.5*sum((y-x-H(y-x)).^2)+lam0*theta(x)+sum(phi(w.*(D*x)));end, f=y-x-H(y-x); end
function [A,B]=BAfilt(d,fc,N), b1=[1 -1];for i=1:d-1,b1=conv(b1,[-1 2 -1]);end;b=conv(b1,[-1 1]);omc=2*pi*fc;t=((1-cos(omc))/(1+cos(omc)))^d;a=1;for i=1:d,a=conv(a,[1 2 1]);end;a=b+t*a;A=spdiags(a(ones(N,1),:),-d:d,N,N);B=spdiags(b(ones(N,1),:),-d:d,N,N); end